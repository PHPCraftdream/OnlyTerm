#![cfg(target_os = "macos")]
use crate::ToastNotification;
use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSError, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::sync::{LazyLock, Once};

const NEEDS_SIGN: &str = "Note that the application must be code-signed \
                          for UNUserNotificationCenter to work";

fn ns_error_to_string(err: *mut NSError) -> String {
    if err.is_null() {
        "null error".to_string()
    } else {
        // SAFETY: `err` was checked for null above. It is a raw pointer to an
        // NSError object handed to us by an Objective-C completion-handler
        // callback, which retains the object for the duration of the call, so
        // creating a temporary shared reference to invoke read-only accessor
        // methods is sound.
        unsafe {
            let err: &NSError = &*err;
            format!(
                "{} {:?}",
                err.localizedDescription(),
                err.localizedFailureReason()
            )
        }
    }
}

define_class!(
    // SAFETY: `#[unsafe(super = NSObject)]` declares that WezTermNotifDelegate
    // inherits from the real Foundation `NSObject` base class, the valid root
    // superclass for a new Objective-C class.
    #[unsafe(super = NSObject)]
    #[name = "WezTermNotifDelegate"]
    #[derive(Debug)]
    struct NotifDelegate;

    // SAFETY: NotifDelegate derives from NSObject, which itself conforms to the
    // NSObjectProtocol, so claiming conformance here is valid.
    unsafe impl NSObjectProtocol for NotifDelegate {}

    // SAFETY: NotifDelegate implements the selectors required by the
    // UNUserNotificationCenterDelegate protocol; the two method attributes below
    // map to the real delegate selectors with matching Objective-C signatures.
    unsafe impl UNUserNotificationCenterDelegate for NotifDelegate {
        // SAFETY: this attribute maps to the real selector
        // `- [UNUserNotificationCenterDelegate userNotificationCenter:willPresentNotification:withCompletionHandler:]`
        // and the Rust argument types match the documented Objective-C parameter types.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        unsafe fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::Block<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            log::debug!("will_present");
            let options = UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound
                | UNNotificationPresentationOptions::Badge
                | UNNotificationPresentationOptions::Banner;
            completion_handler.call((options,));
        }

        // SAFETY: this attribute maps to the real selector
        // `- [UNUserNotificationCenterDelegate userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:]`
        // and the Rust argument types match the documented Objective-C parameter types.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        unsafe fn did_receive_notification(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &Block<dyn Fn()>,
        ) {
            let action = response.actionIdentifier();
            let user_info = response.notification().request().content().userInfo();
            let url = user_info.valueForKey(ns_string!("url"));

            log::debug!("did_receive_notification -> action={action:?} url={url:?}");

            if let Some(url) = url {
                if let Ok(url_str) = url.downcast::<NSString>() {
                    wezterm_open_url::open_url(&url_str.to_string());
                }
            }

            completion_handler.call(());
        }
    }
);

impl NotifDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: `this` is a freshly allocated NotifDelegate instance. Sending
        // `[super init]` is the standard NSObject designated initializer and
        // completes initialization of the NSObject superclass.
        let me: Retained<Self> = unsafe { msg_send![super(this), init] };
        log::debug!("new delegate {:?}", Retained::as_ptr(&me));
        me
    }
}

impl Drop for NotifDelegate {
    fn drop(&mut self) {
        log::debug!("dropping NotifDelegate {:?}", self as *mut Self);
    }
}

const CENTER: LazyLock<Retained<UNUserNotificationCenter>> = LazyLock::new(|| {
    // SAFETY: `currentNotificationCenter` is a thread-safe factory class method
    // returning the process-wide shared UNUserNotificationCenter singleton.
    unsafe { UNUserNotificationCenter::currentNotificationCenter() }
});

pub fn initialize() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        // SAFETY: the following are Objective-C message sends targeting the
        // shared, owned `CENTER` singleton and freshly allocated, owned
        // UNNotificationAction/UNNotificationCategory objects. Block arguments
        // are wrapped in `RcBlock` with matching signatures, and string arguments
        // are `ns_string!`/`NSString` values that live for the call. This runs
        // exactly once thanks to `Once`.
        CENTER.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Provisional
                | UNAuthorizationOptions::Sound,
            &RcBlock::new(|ok: Bool, err| {
                if ok.is_false() {
                    log::error!(
                        "requestAuthorization status={ok:?} {}. {NEEDS_SIGN}",
                        ns_error_to_string(err)
                    );
                }
            }),
        );

        let show_url = UNNotificationAction::actionWithIdentifier_title_options(
            ns_string!("SHOW_URL"),
            ns_string!("Show"),
            UNNotificationActionOptions::empty(),
        );
        let show_url_cat =
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                ns_string!("SHOW_URL_ACTION"),
                &NSArray::from_retained_slice(&[show_url]),
                &NSArray::from_slice(&[]),
                UNNotificationCategoryOptions::CustomDismissAction,
            );
        CENTER.setNotificationCategories(&NSSet::from_retained_slice(&[show_url_cat]));

        let delegate = NotifDelegate::new();
        let delegate_proto = ProtocolObject::from_retained(delegate.clone());
        CENTER.setDelegate(Some(&delegate_proto));
        log::debug!(
            "after setDelegate {:?}, center.delegate={:?}",
            delegate,
            CENTER.delegate()
        );

        // Intentionally "leak" the delegate.
        // I've tried stashing it into a global to keep it alive,
        // but something still manages to drop the underlying delegate
        // and that will break the weak ref in the center.
        // This is likely not the right way to do this, but after
        // spending two hours scratching my head, this is the least
        // crazy thing.
        Retained::into_raw(delegate);
    });
}

pub fn show_notif(toast: ToastNotification) -> Result<(), Box<dyn std::error::Error>> {
    initialize();
    // SAFETY: the calls below target the shared, already-initialized `CENTER`
    // singleton and freshly allocated, owned UNMutableNotificationContent /
    // UNNotificationRequest objects. String arguments are owned NSStrings valid
    // for the call, and the completion-handler block is wrapped in an `RcBlock`
    // with a matching signature.
    unsafe {
        log::debug!("show_notif center.delegate is {:?}", CENTER.delegate());

        let notif = UNMutableNotificationContent::new();
        notif.setTitle(&NSString::from_str(&toast.title));
        notif.setBody(&NSString::from_str(&toast.message));

        if let Some(url) = &toast.url {
            let info =
                NSDictionary::from_slices(&[ns_string!("url")], &[&*NSString::from_str(&url)]);
            notif.setUserInfo(
                info.downcast_ref::<NSDictionary>()
                    .expect("is NSDictionary"),
            );
            notif.setCategoryIdentifier(ns_string!("SHOW_URL_ACTION"));
        }

        let identifier = uuid::Uuid::new_v4().to_string();
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier),
            &*notif,
            None,
        );

        CENTER.addNotificationRequest_withCompletionHandler(
            &*request,
            Some(&RcBlock::new(move |err: *mut NSError| {
                if err.is_null() {
                    if let Some(timeout) = toast.timeout {
                        // Spawn a thread to wait. This could be more efficient.
                        // We cannot simply use performSelector:withObject:afterDelay:
                        // because we're not guaranteed to be called from the main
                        // thread.  We also don't have access to the executor machinery
                        // from the window crate here, so we just do this basic take.
                        let identifier = identifier.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(timeout);
                            // Remove this notification
                            let ident_array =
                                NSArray::from_retained_slice(&[NSString::from_str(&identifier)]);
                            CENTER.removeDeliveredNotificationsWithIdentifiers(&ident_array);
                        });
                    }
                } else {
                    log::error!("notif failed {}. {NEEDS_SIGN}", ns_error_to_string(err));
                }
            })),
        );
    }

    Ok(())
}

#[cfg(any(feature = "std", feature = "alloc"))]
use crate::{CsiParam, VTActor};
#[cfg(any(feature = "std", feature = "alloc"))]
use alloc::vec::Vec;

/// `VTAction` is an alternative way to work with the parser; rather
/// than implementing the VTActor trait you can use `CollectingVTActor`
/// to capture the sequence of events into a `Vec<VTAction>`.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum VTAction {
    Print(char),
    ExecuteC0orC1(u8),
    DcsHook {
        params: Vec<i64>,
        intermediates: Vec<u8>,
        ignored_excess_intermediates: bool,
        byte: u8,
    },
    DcsPut(u8),
    DcsUnhook,
    EscDispatch {
        params: Vec<i64>,
        intermediates: Vec<u8>,
        ignored_excess_intermediates: bool,
        byte: u8,
    },
    CsiDispatch {
        params: Vec<CsiParam>,
        parameters_truncated: bool,
        byte: u8,
    },
    OscDispatch(Vec<Vec<u8>>),
    ApcDispatch(Vec<u8>),
}

/// This is an implementation of `VTActor` that captures the events
/// into an internal vector.
/// It can be iterated via `into_iter` or have the internal
/// vector extracted via `into_vec`.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Default)]
pub struct CollectingVTActor {
    actions: Vec<VTAction>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl IntoIterator for CollectingVTActor {
    type Item = VTAction;
    type IntoIter = alloc::vec::IntoIter<VTAction>;

    fn into_iter(self) -> Self::IntoIter {
        self.actions.into_iter()
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl CollectingVTActor {
    pub fn into_vec(self) -> Vec<VTAction> {
        self.actions
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl VTActor for CollectingVTActor {
    fn print(&mut self, b: char) {
        self.actions.push(VTAction::Print(b));
    }

    fn execute_c0_or_c1(&mut self, control: u8) {
        self.actions.push(VTAction::ExecuteC0orC1(control));
    }

    fn dcs_hook(
        &mut self,
        byte: u8,
        params: &[i64],
        intermediates: &[u8],
        ignored_excess_intermediates: bool,
    ) {
        self.actions.push(VTAction::DcsHook {
            byte,
            params: params.to_vec(),
            intermediates: intermediates.to_vec(),
            ignored_excess_intermediates,
        });
    }

    fn dcs_put(&mut self, byte: u8) {
        self.actions.push(VTAction::DcsPut(byte));
    }

    fn dcs_unhook(&mut self) {
        self.actions.push(VTAction::DcsUnhook);
    }

    fn esc_dispatch(
        &mut self,
        params: &[i64],
        intermediates: &[u8],
        ignored_excess_intermediates: bool,
        byte: u8,
    ) {
        self.actions.push(VTAction::EscDispatch {
            params: params.to_vec(),
            intermediates: intermediates.to_vec(),
            ignored_excess_intermediates,
            byte,
        });
    }

    fn csi_dispatch(&mut self, params: &[CsiParam], parameters_truncated: bool, byte: u8) {
        self.actions.push(VTAction::CsiDispatch {
            params: params.to_vec(),
            parameters_truncated,
            byte,
        });
    }

    fn osc_dispatch(&mut self, params: &[&[u8]]) {
        self.actions.push(VTAction::OscDispatch(
            params.iter().map(|i| i.to_vec()).collect(),
        ));
    }

    fn apc_dispatch(&mut self, data: Vec<u8>) {
        self.actions.push(VTAction::ApcDispatch(data));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RendererInfoSource<T> {
    Hosted(T),
    Context(usize),
    Unavailable,
}

pub(super) fn select_source<T>(
    hosted: Option<T>,
    context_identity: Option<usize>,
    unavailable: bool,
) -> Option<RendererInfoSource<T>> {
    if let Some(info) = hosted {
        Some(RendererInfoSource::Hosted(info))
    } else if let Some(identity) = context_identity {
        Some(RendererInfoSource::Context(identity))
    } else if unavailable {
        Some(RendererInfoSource::Unavailable)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub(super) struct RendererInfoCache<T> {
    source: Option<RendererInfoSource<T>>,
    rendered: Option<String>,
}

impl<T> Default for RendererInfoCache<T> {
    fn default() -> Self {
        Self {
            source: None,
            rendered: None,
        }
    }
}

impl<T: PartialEq + Clone> RendererInfoCache<T> {
    pub(super) fn reset(&mut self) {
        self.source = None;
        self.rendered = None;
    }

    pub(super) fn update(
        &mut self,
        source: Option<RendererInfoSource<T>>,
        format_hosted: impl FnOnce(T) -> String,
        format_context: impl FnOnce() -> String,
    ) -> Option<&str> {
        if self.source == source {
            return None;
        }
        self.rendered = source.as_ref().map(|selected| match selected {
            RendererInfoSource::Hosted(info) => format_hosted(info.clone()),
            RendererInfoSource::Context(_) => format_context(),
            RendererInfoSource::Unavailable => "WebGPU: n/a".to_string(),
        });
        self.source = source;
        self.rendered.as_deref()
    }
}

#[cfg(test)]
mod renderer_info_cache_tests {
    use super::{select_source, RendererInfoCache, RendererInfoSource};

    #[test]
    fn caches_and_selects_hosted_then_local_then_unavailable() {
        let mut cache = RendererInfoCache::default();
        let formatted = std::cell::Cell::new(0);
        let hosted_key = select_source(Some("host"), Some(11), false);
        assert_eq!(hosted_key, Some(RendererInfoSource::Hosted("host")));
        let output = |cache: &mut RendererInfoCache<&str>, source| {
            cache
                .update(
                    source,
                    |info| {
                        formatted.set(formatted.get() + 1);
                        format!("WebGPU: {info}")
                    },
                    || {
                        formatted.set(formatted.get() + 1);
                        "WebGPU: local".to_string()
                    },
                )
                .map(str::to_owned)
        };
        assert_eq!(
            output(&mut cache, hosted_key.clone()).as_deref(),
            Some("WebGPU: host")
        );
        assert_eq!(output(&mut cache, hosted_key.clone()).as_deref(), None);
        assert_eq!(formatted.get(), 1);
        let changed_host = select_source(Some("host-2"), Some(11), false);
        assert_eq!(
            output(&mut cache, changed_host.clone()).as_deref(),
            Some("WebGPU: host-2")
        );
        assert_eq!(formatted.get(), 2);
        let local_id = 11usize;
        let local = select_source(None::<&str>, Some(local_id), false);
        assert_eq!(local, Some(RendererInfoSource::Context(local_id)));
        assert_eq!(
            output(&mut cache, local.clone()).as_deref(),
            Some("WebGPU: local")
        );
        assert_eq!(output(&mut cache, local).as_deref(), None);
        assert_eq!(formatted.get(), 3);
        let context_changed = select_source(None::<&str>, Some(12), false);
        assert_eq!(
            output(&mut cache, context_changed.clone()).as_deref(),
            Some("WebGPU: local")
        );
        assert_eq!(formatted.get(), 4);
        let unavailable = select_source(None::<&str>, None, true);
        assert_eq!(
            output(&mut cache, unavailable.clone()).as_deref(),
            Some("WebGPU: n/a")
        );
        assert_eq!(output(&mut cache, unavailable).as_deref(), None);
        assert_eq!(formatted.get(), 4);
        assert_eq!(
            output(&mut cache, hosted_key).as_deref(),
            Some("WebGPU: host")
        );
        assert_eq!(formatted.get(), 5);
    }
}

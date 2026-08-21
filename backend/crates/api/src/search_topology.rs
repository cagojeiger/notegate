use notegate_core::ProcessMode;

/// Search components and observability ownership for one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchTopology<'a> {
    pub(crate) public_listener: bool,
    pub(crate) search_listener: bool,
    pub(crate) client: SearchClientTarget<'a>,
    pub(crate) metrics_owner: SearchMetricsOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchClientTarget<'a> {
    /// Call the process-local private listener over signed HTTP.
    LocalListener,
    /// Call a separately deployed private search service.
    RemoteService(&'a str),
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchMetricsOwner {
    PublicListener,
    SearchListener,
    None,
}

impl<'a> SearchTopology<'a> {
    pub(crate) fn plan(process_mode: ProcessMode, search_service_url: Option<&'a str>) -> Self {
        let public_listener = process_mode.runs_api() || process_mode.runs_worker();
        let search_listener = process_mode == ProcessMode::Search
            || (process_mode.serves_search() && search_service_url.is_none());
        let client = if !process_mode.runs_api() {
            SearchClientTarget::Disabled
        } else if let Some(url) = search_service_url {
            SearchClientTarget::RemoteService(url)
        } else {
            SearchClientTarget::LocalListener
        };
        let metrics_owner = if process_mode == ProcessMode::Search {
            SearchMetricsOwner::SearchListener
        } else if search_listener && process_mode.runs_api() {
            SearchMetricsOwner::PublicListener
        } else {
            SearchMetricsOwner::None
        };

        Self {
            public_listener,
            search_listener,
            client,
            metrics_owner,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REMOTE: &str = "http://notegate-search:9192";

    #[test]
    fn process_mode_and_service_url_define_search_topology() {
        let cases = [
            (
                ProcessMode::All,
                None,
                SearchTopology {
                    public_listener: true,
                    search_listener: true,
                    client: SearchClientTarget::LocalListener,
                    metrics_owner: SearchMetricsOwner::PublicListener,
                },
            ),
            (
                ProcessMode::All,
                Some(REMOTE),
                SearchTopology {
                    public_listener: true,
                    search_listener: false,
                    client: SearchClientTarget::RemoteService(REMOTE),
                    metrics_owner: SearchMetricsOwner::None,
                },
            ),
            (
                ProcessMode::Api,
                None,
                SearchTopology {
                    public_listener: true,
                    search_listener: true,
                    client: SearchClientTarget::LocalListener,
                    metrics_owner: SearchMetricsOwner::PublicListener,
                },
            ),
            (
                ProcessMode::Api,
                Some(REMOTE),
                SearchTopology {
                    public_listener: true,
                    search_listener: false,
                    client: SearchClientTarget::RemoteService(REMOTE),
                    metrics_owner: SearchMetricsOwner::None,
                },
            ),
            (
                ProcessMode::Worker,
                None,
                SearchTopology {
                    public_listener: true,
                    search_listener: false,
                    client: SearchClientTarget::Disabled,
                    metrics_owner: SearchMetricsOwner::None,
                },
            ),
            (
                ProcessMode::Worker,
                Some(REMOTE),
                SearchTopology {
                    public_listener: true,
                    search_listener: false,
                    client: SearchClientTarget::Disabled,
                    metrics_owner: SearchMetricsOwner::None,
                },
            ),
            (
                ProcessMode::Search,
                None,
                SearchTopology {
                    public_listener: false,
                    search_listener: true,
                    client: SearchClientTarget::Disabled,
                    metrics_owner: SearchMetricsOwner::SearchListener,
                },
            ),
            (
                ProcessMode::Search,
                Some(REMOTE),
                SearchTopology {
                    public_listener: false,
                    search_listener: true,
                    client: SearchClientTarget::Disabled,
                    metrics_owner: SearchMetricsOwner::SearchListener,
                },
            ),
        ];

        for (process_mode, service_url, expected) in cases {
            assert_eq!(
                SearchTopology::plan(process_mode, service_url),
                expected,
                "process_mode={} service_url={service_url:?}",
                process_mode.as_str(),
            );
        }
    }
}

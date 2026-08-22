use notegate_core::ProcessMode;

/// Process-local components and listener ownership derived from runtime config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimePlan<'a> {
    pub(crate) runs_api: bool,
    pub(crate) runs_worker: bool,
    pub(crate) runs_reconciler: bool,
    pub(crate) main_listener: bool,
    pub(crate) search_listener: bool,
    pub(crate) search_client: SearchClientTarget<'a>,
    pub(crate) metrics_endpoint: ListenerSurface,
    pub(crate) uses_read_pool: bool,
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
pub(crate) enum ListenerSurface {
    Main,
    Search,
}

impl<'a> RuntimePlan<'a> {
    pub(crate) fn from_config(
        process_mode: ProcessMode,
        search_service_url: Option<&'a str>,
    ) -> Self {
        let runs_api = process_mode.runs_api();
        let runs_worker = process_mode.runs_worker();
        let runs_reconciler = process_mode.runs_reconciler();
        let main_listener = process_mode.exposes_public_listener();
        let search_listener = process_mode == ProcessMode::Search
            || (process_mode.serves_search() && search_service_url.is_none());
        let search_client = if !runs_api {
            SearchClientTarget::Disabled
        } else if let Some(url) = search_service_url {
            SearchClientTarget::RemoteService(url)
        } else {
            SearchClientTarget::LocalListener
        };
        let metrics_endpoint = if process_mode == ProcessMode::Search {
            ListenerSurface::Search
        } else {
            ListenerSurface::Main
        };

        Self {
            runs_api,
            runs_worker,
            runs_reconciler,
            main_listener,
            search_listener,
            search_client,
            metrics_endpoint,
            uses_read_pool: search_listener,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REMOTE: &str = "http://notegate-search:9192";

    #[derive(Debug, Clone, Copy)]
    struct Expected {
        api: bool,
        worker: bool,
        reconciler: bool,
        main_listener: bool,
        search_listener: bool,
        search_client: SearchClientTarget<'static>,
        metrics_endpoint: ListenerSurface,
    }

    #[test]
    fn process_mode_and_search_target_define_the_complete_runtime_plan() {
        let cases = [
            (
                ProcessMode::All,
                None,
                Expected {
                    api: true,
                    worker: true,
                    reconciler: true,
                    main_listener: true,
                    search_listener: true,
                    search_client: SearchClientTarget::LocalListener,
                    metrics_endpoint: ListenerSurface::Main,
                },
            ),
            (
                ProcessMode::All,
                Some(REMOTE),
                Expected {
                    api: true,
                    worker: true,
                    reconciler: true,
                    main_listener: true,
                    search_listener: false,
                    search_client: SearchClientTarget::RemoteService(REMOTE),
                    metrics_endpoint: ListenerSurface::Main,
                },
            ),
            (
                ProcessMode::Api,
                None,
                Expected {
                    api: true,
                    worker: false,
                    reconciler: false,
                    main_listener: true,
                    search_listener: true,
                    search_client: SearchClientTarget::LocalListener,
                    metrics_endpoint: ListenerSurface::Main,
                },
            ),
            (
                ProcessMode::Api,
                Some(REMOTE),
                Expected {
                    api: true,
                    worker: false,
                    reconciler: false,
                    main_listener: true,
                    search_listener: false,
                    search_client: SearchClientTarget::RemoteService(REMOTE),
                    metrics_endpoint: ListenerSurface::Main,
                },
            ),
            (
                ProcessMode::Worker,
                None,
                Expected {
                    api: false,
                    worker: true,
                    reconciler: false,
                    main_listener: true,
                    search_listener: false,
                    search_client: SearchClientTarget::Disabled,
                    metrics_endpoint: ListenerSurface::Main,
                },
            ),
            (
                ProcessMode::Reconciler,
                None,
                Expected {
                    api: false,
                    worker: false,
                    reconciler: true,
                    main_listener: true,
                    search_listener: false,
                    search_client: SearchClientTarget::Disabled,
                    metrics_endpoint: ListenerSurface::Main,
                },
            ),
            (
                ProcessMode::Search,
                None,
                Expected {
                    api: false,
                    worker: false,
                    reconciler: false,
                    main_listener: false,
                    search_listener: true,
                    search_client: SearchClientTarget::Disabled,
                    metrics_endpoint: ListenerSurface::Search,
                },
            ),
        ];

        for (process_mode, service_url, expected) in cases {
            let plan = RuntimePlan::from_config(process_mode, service_url);
            assert_eq!(plan.runs_api, expected.api);
            assert_eq!(plan.runs_worker, expected.worker);
            assert_eq!(plan.runs_reconciler, expected.reconciler);
            assert_eq!(plan.main_listener, expected.main_listener);
            assert_eq!(plan.search_listener, expected.search_listener);
            assert_eq!(plan.search_client, expected.search_client);
            assert_eq!(plan.metrics_endpoint, expected.metrics_endpoint);
            assert_eq!(plan.uses_read_pool, expected.search_listener);
        }
    }

    #[test]
    fn irrelevant_search_url_does_not_enable_search_in_background_processes() {
        for process_mode in [ProcessMode::Worker, ProcessMode::Reconciler] {
            let plan = RuntimePlan::from_config(process_mode, Some(REMOTE));
            assert!(!plan.search_listener);
            assert_eq!(plan.search_client, SearchClientTarget::Disabled);
            assert!(!plan.uses_read_pool);
        }

        let search = RuntimePlan::from_config(ProcessMode::Search, Some(REMOTE));
        assert!(search.search_listener);
        assert_eq!(search.search_client, SearchClientTarget::Disabled);
        assert!(search.uses_read_pool);
    }
}

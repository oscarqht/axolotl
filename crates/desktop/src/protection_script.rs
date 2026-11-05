//! Serializes asynchronous document-script replacement without stale navigation.
#[derive(Default)]
pub struct ScriptRegistration {
    generation: u64,
    installed: Option<String>,
    pending_url: Option<String>,
    reload: bool,
}
pub enum Completion {
    Stale(String),
    Current {
        old: Option<String>,
        url: Option<String>,
        reload: bool,
    },
}
impl ScriptRegistration {
    pub fn defer_navigation(&mut self, url: String) -> bool {
        if self.installed.is_none() {
            self.pending_url = Some(url);
            true
        } else {
            false
        }
    }
    pub fn begin(&mut self, url: Option<String>, reload: bool) -> u64 {
        self.generation += 1;
        if url.is_some() {
            self.pending_url = url;
        }
        self.reload |= reload;
        self.generation
    }
    pub fn complete(&mut self, generation: u64, id: String) -> Completion {
        if generation != self.generation {
            return Completion::Stale(id);
        }
        Completion::Current {
            old: self.installed.replace(id),
            url: self.pending_url.take(),
            reload: std::mem::take(&mut self.reload),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn superseding_initial_script_retains_destination_and_removes_stale_script() {
        let mut state = ScriptRegistration::default();
        let initial = state.begin(Some("https://example.org/".into()), false);
        assert!(state.defer_navigation("https://latest.example/".into()));
        let toggle = state.begin(None, true);
        assert!(
            matches!(state.complete(initial,"stale".into()),Completion::Stale(id) if id=="stale")
        );
        assert!(
            matches!(state.complete(toggle,"current".into()),Completion::Current{old:None,url:Some(url),reload:true} if url=="https://latest.example/")
        );
        let update = state.begin(None, false);
        assert!(
            matches!(state.complete(update,"updated".into()),Completion::Current{old:Some(id),url:None,reload:false} if id=="current")
        );
    }
    #[test]
    fn out_of_order_completion_never_replaces_current_policy() {
        let mut state = ScriptRegistration::default();
        let old = state.begin(None, true);
        let current = state.begin(None, false);
        assert!(matches!(
            state.complete(current, "newest".into()),
            Completion::Current { reload: true, .. }
        ));
        assert!(matches!(state.complete(old,"older".into()),Completion::Stale(id) if id=="older"));
        let next = state.begin(None, false);
        assert!(
            matches!(state.complete(next,"next".into()),Completion::Current{old:Some(id),reload:false,..} if id=="newest")
        );
    }
}

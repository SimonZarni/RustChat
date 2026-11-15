use std::{collections::HashMap, sync::Mutex};

use uuid::Uuid;

#[derive(Default)]
pub(crate) struct PresenceTracker(Mutex<HashMap<Uuid, usize>>);

impl PresenceTracker {
    /// Returns true only when this is the user's first live socket.
    pub(crate) fn connect(&self, user: Uuid) -> bool {
        let mut users = self.0.lock().expect("presence registry poisoned");
        let count = users.entry(user).or_default();
        let became_online = *count == 0;
        *count = count.saturating_add(1);
        became_online
    }

    /// Returns true only when the user's last live socket closed.
    pub(crate) fn disconnect(&self, user: Uuid) -> bool {
        let mut users = self.0.lock().expect("presence registry poisoned");
        let Some(count) = users.get_mut(&user) else {
            return false;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            users.remove(&user);
            true
        } else {
            false
        }
    }

    pub(crate) fn is_online(&self, user: Uuid) -> bool {
        self.0
            .lock()
            .expect("presence registry poisoned")
            .contains_key(&user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_sockets_produce_one_online_and_one_offline_transition() {
        let tracker = PresenceTracker::default();
        let user = Uuid::now_v7();
        assert!(tracker.connect(user));
        assert!(!tracker.connect(user));
        assert!(tracker.is_online(user));
        assert!(!tracker.disconnect(user));
        assert!(tracker.is_online(user));
        assert!(tracker.disconnect(user));
        assert!(!tracker.is_online(user));
    }
}

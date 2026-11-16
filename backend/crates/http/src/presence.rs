use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use rustchat_application::ports::PresenceStore;
use rustchat_domain::identity::UserId;
use uuid::Uuid;

/// Which users have a live socket. Counts are kept per process; with a shared
/// [`PresenceStore`] the "online" and "offline" transitions and the status
/// snapshots take every instance into account.
#[derive(Default)]
pub(crate) struct PresenceTracker {
    local: Mutex<HashMap<Uuid, usize>>,
    shared: Option<Arc<dyn PresenceStore>>,
}

impl PresenceTracker {
    pub(crate) fn shared(store: Arc<dyn PresenceStore>) -> Self {
        Self {
            local: Mutex::default(),
            shared: Some(store),
        }
    }

    /// Returns true only when the user was not already online (here or, with a
    /// shared store, on another instance).
    pub(crate) async fn connect(&self, user: Uuid) -> bool {
        let first_here = {
            let mut users = self.local.lock().expect("presence registry poisoned");
            let count = users.entry(user).or_default();
            let first = *count == 0;
            *count = count.saturating_add(1);
            first
        };
        if !first_here {
            return false;
        }
        match &self.shared {
            Some(store) => match store.connect(UserId::from_uuid(user)).await {
                Ok(first_anywhere) => first_anywhere,
                Err(error) => {
                    tracing::warn!(%error, "could not record shared presence");
                    true
                }
            },
            None => true,
        }
    }

    /// Returns true only when the user's last live socket closed (everywhere,
    /// with a shared store).
    pub(crate) async fn disconnect(&self, user: Uuid) -> bool {
        let last_here = {
            let mut users = self.local.lock().expect("presence registry poisoned");
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
        };
        if !last_here {
            return false;
        }
        match &self.shared {
            Some(store) => match store.disconnect(UserId::from_uuid(user)).await {
                Ok(last_anywhere) => last_anywhere,
                Err(error) => {
                    tracing::warn!(%error, "could not clear shared presence");
                    true
                }
            },
            None => true,
        }
    }

    /// The members of `users` that are online.
    pub(crate) async fn online_among(&self, users: &[Uuid]) -> HashSet<Uuid> {
        let mut online: HashSet<Uuid> = {
            let local = self.local.lock().expect("presence registry poisoned");
            users
                .iter()
                .copied()
                .filter(|user| local.contains_key(user))
                .collect()
        };
        if let Some(store) = &self.shared {
            let remaining: Vec<UserId> = users
                .iter()
                .filter(|user| !online.contains(user))
                .map(|user| UserId::from_uuid(*user))
                .collect();
            match store.online(&remaining).await {
                Ok(found) => online.extend(found.into_iter().map(|user| user.as_uuid())),
                Err(error) => tracing::warn!(%error, "could not read shared presence"),
            }
        }
        online
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn multiple_sockets_produce_one_online_and_one_offline_transition() {
        let tracker = PresenceTracker::default();
        let user = Uuid::now_v7();
        assert!(tracker.connect(user).await);
        assert!(!tracker.connect(user).await);
        assert!(tracker.online_among(&[user]).await.contains(&user));
        assert!(!tracker.disconnect(user).await);
        assert!(tracker.online_among(&[user]).await.contains(&user));
        assert!(tracker.disconnect(user).await);
        assert!(tracker.online_among(&[user]).await.is_empty());
    }

    #[tokio::test]
    async fn disconnecting_an_unknown_user_is_not_a_transition() {
        assert!(!PresenceTracker::default().disconnect(Uuid::now_v7()).await);
    }
}

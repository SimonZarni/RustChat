//! Conversation creation, listing and membership use cases.

use std::sync::Arc;

use rustchat_domain::{
    conversation::{Conversation, ConversationPage},
    identity::{ConversationId, UserId, Username},
};

use crate::{
    error::AppError,
    ports::{ConversationRepository, UserRepository},
};

#[derive(Debug, Clone, Copy)]
pub struct ConversationConfig {
    pub max_members: usize,
    pub max_page_size: usize,
}

pub struct ConversationService {
    users: Arc<dyn UserRepository>,
    conversations: Arc<dyn ConversationRepository>,
    config: ConversationConfig,
}

impl ConversationService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        conversations: Arc<dyn ConversationRepository>,
        config: ConversationConfig,
    ) -> Self {
        Self {
            users,
            conversations,
            config,
        }
    }

    pub async fn create_direct(
        &self,
        caller: UserId,
        username: &str,
    ) -> Result<Conversation, AppError> {
        let username = Username::parse(username)?;
        let target = self
            .users
            .find_discoverable(caller, &username)
            .await?
            .ok_or(AppError::NotFound)?;
        if caller == target.id {
            return Err(AppError::InvalidConversationRequest);
        }
        self.conversations
            .create_or_get_direct(caller, target.id)
            .await
    }

    pub async fn create_group(
        &self,
        caller: UserId,
        usernames: &[String],
    ) -> Result<Conversation, AppError> {
        if usernames.is_empty() || usernames.len().saturating_add(1) > self.config.max_members {
            return Err(AppError::ConversationMemberLimit);
        }
        let mut members = vec![caller];
        for raw in usernames {
            let username = Username::parse(raw)?;
            let user = self
                .users
                .find_discoverable(caller, &username)
                .await?
                .ok_or(AppError::NotFound)?;
            if members.contains(&user.id) {
                return Err(AppError::InvalidConversationRequest);
            }
            members.push(user.id);
        }
        self.conversations
            .create_group(caller, members, self.config.max_members)
            .await
    }

    pub async fn add_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        username: &str,
    ) -> Result<(), AppError> {
        let username = Username::parse(username)?;
        let user = self
            .users
            .find_discoverable(caller, &username)
            .await?
            .ok_or(AppError::NotFound)?;
        self.conversations
            .add_member(caller, conversation, user.id, self.config.max_members)
            .await
    }

    pub async fn remove_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        member: UserId,
    ) -> Result<(), AppError> {
        self.conversations
            .remove_member(caller, conversation, member)
            .await
    }

    pub async fn list(
        &self,
        caller: UserId,
        before: Option<ConversationId>,
        limit: usize,
    ) -> Result<ConversationPage, AppError> {
        self.conversations
            .list_for_member(caller, before, limit.clamp(1, self.config.max_page_size))
            .await
    }

    pub async fn get(&self, caller: UserId, id: ConversationId) -> Result<Conversation, AppError> {
        self.conversations.find_for_member(caller, id).await
    }

    pub async fn members(
        &self,
        caller: UserId,
        id: ConversationId,
    ) -> Result<Vec<rustchat_domain::identity::User>, AppError> {
        self.conversations.members_for_member(caller, id).await
    }

    pub async fn members_presence(
        &self,
        caller: UserId,
        id: ConversationId,
    ) -> Result<Vec<(rustchat_domain::identity::User, bool)>, AppError> {
        self.conversations
            .members_presence_for_member(caller, id)
            .await
    }

    pub async fn share_conversation(
        &self,
        first: UserId,
        second: UserId,
    ) -> Result<bool, AppError> {
        self.conversations.share_conversation(first, second).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rustchat_domain::{
        conversation::ConversationKind,
        identity::{ProfilePrivacy, User, Username},
    };
    use std::sync::Mutex;
    use time::OffsetDateTime;

    #[derive(Default)]
    struct Users(Vec<User>);

    #[async_trait]
    impl crate::ports::UserRepository for Users {
        async fn create(
            &self,
            _: crate::ports::NewUser,
        ) -> Result<crate::ports::CreateUserOutcome, AppError> {
            Ok(crate::ports::CreateUserOutcome::Created)
        }
        async fn find_credentials(
            &self,
            _: &Username,
        ) -> Result<Option<crate::ports::UserCredentials>, AppError> {
            Ok(None)
        }
        async fn find_discoverable(
            &self,
            _caller: UserId,
            username: &Username,
        ) -> Result<Option<User>, AppError> {
            Ok(self
                .0
                .iter()
                .find(|user| &user.username == username)
                .cloned())
        }
        async fn find_profile_for_caller(
            &self,
            _caller: UserId,
            _: &Username,
        ) -> Result<Option<rustchat_domain::identity::UserProfile>, AppError> {
            Ok(None)
        }
        async fn get_privacy(&self, _: UserId) -> Result<ProfilePrivacy, AppError> {
            Ok(ProfilePrivacy::default())
        }
        async fn update_privacy(
            &self,
            _: UserId,
            privacy: ProfilePrivacy,
        ) -> Result<ProfilePrivacy, AppError> {
            Ok(privacy)
        }
        async fn deactivate_account(&self, _: UserId, _: OffsetDateTime) -> Result<(), AppError> {
            Ok(())
        }
        async fn delete_account(
            &self,
            _: UserId,
            _: &Username,
            _: &str,
            _: OffsetDateTime,
        ) -> Result<(), AppError> {
            Ok(())
        }
        async fn email_status(&self, _: UserId) -> Result<crate::ports::EmailStatus, AppError> {
            unreachable!("not used by conversation tests")
        }
        async fn set_email(
            &self,
            _: UserId,
            _: Option<&rustchat_domain::identity::EmailAddress>,
        ) -> Result<(), AppError> {
            unreachable!("not used by conversation tests")
        }
        async fn issue_email_verification_token(
            &self,
            _: UserId,
            _: &rustchat_domain::identity::EmailAddress,
            _: crate::ports::TokenDigest,
            _: OffsetDateTime,
            _: OffsetDateTime,
        ) -> Result<bool, AppError> {
            unreachable!("not used by conversation tests")
        }
        async fn verify_email_with_token(
            &self,
            _: crate::ports::TokenDigest,
            _: OffsetDateTime,
        ) -> Result<crate::ports::VerifyEmailOutcome, AppError> {
            unreachable!("not used by conversation tests")
        }
        async fn issue_password_reset_token(
            &self,
            _: &rustchat_domain::identity::EmailAddress,
            _: crate::ports::TokenDigest,
            _: OffsetDateTime,
            _: OffsetDateTime,
        ) -> Result<bool, AppError> {
            Ok(false)
        }
        async fn reset_password_with_token(
            &self,
            _: crate::ports::TokenDigest,
            _: String,
            _: OffsetDateTime,
        ) -> Result<bool, AppError> {
            Ok(false)
        }
        async fn list_admin_users(&self, _: i64) -> Result<Vec<crate::ports::AdminUser>, AppError> {
            Ok(Vec::new())
        }
        async fn set_user_suspended(
            &self,
            _: UserId,
            _: UserId,
            _: bool,
            _: OffsetDateTime,
        ) -> Result<crate::ports::AdminUser, AppError> {
            Err(AppError::NotFound)
        }
        async fn update_username(
            &self,
            user_id: UserId,
            username: &Username,
        ) -> Result<User, AppError> {
            self.0
                .iter()
                .find(|user| user.id == user_id)
                .map(|user| User {
                    username: username.clone(),
                    ..user.clone()
                })
                .ok_or(AppError::NotFound)
        }
        async fn get_profile(
            &self,
            user_id: UserId,
        ) -> Result<rustchat_domain::identity::UserProfile, AppError> {
            let user = self
                .0
                .iter()
                .find(|user| user.id == user_id)
                .ok_or(AppError::NotFound)?;
            Ok(rustchat_domain::identity::UserProfile {
                user_id,
                username: user.username.clone(),
                display_name: user.username.to_string(),
                bio: String::new(),
                updated_at: user.created_at,
            })
        }
        async fn update_profile(
            &self,
            user_id: UserId,
            display_name: &str,
            bio: &str,
            now: OffsetDateTime,
        ) -> Result<rustchat_domain::identity::UserProfile, AppError> {
            let user = self
                .0
                .iter()
                .find(|user| user.id == user_id)
                .ok_or(AppError::NotFound)?;
            Ok(rustchat_domain::identity::UserProfile {
                user_id,
                username: user.username.clone(),
                display_name: display_name.to_owned(),
                bio: bio.to_owned(),
                updated_at: now,
            })
        }
    }

    #[derive(Default)]
    struct Conversations(Mutex<Vec<(UserId, UserId)>>);

    #[async_trait]
    impl ConversationRepository for Conversations {
        async fn create_or_get_direct(
            &self,
            caller: UserId,
            target: UserId,
        ) -> Result<Conversation, AppError> {
            self.0.lock().unwrap().push((caller, target));
            Ok(Conversation {
                id: ConversationId::new(),
                kind: ConversationKind::Direct,
                created_by: caller,
                created_at: OffsetDateTime::UNIX_EPOCH,
                members: vec![caller, target],
            })
        }
        async fn create_group(
            &self,
            _: UserId,
            _: Vec<UserId>,
            _: usize,
        ) -> Result<Conversation, AppError> {
            Err(AppError::NotFound)
        }
        async fn add_member(
            &self,
            _: UserId,
            _: ConversationId,
            _: UserId,
            _: usize,
        ) -> Result<(), AppError> {
            Err(AppError::NotFound)
        }
        async fn remove_member(
            &self,
            _: UserId,
            _: ConversationId,
            _: UserId,
        ) -> Result<(), AppError> {
            Err(AppError::NotFound)
        }
        async fn list_for_member(
            &self,
            _: UserId,
            _: Option<ConversationId>,
            _: usize,
        ) -> Result<ConversationPage, AppError> {
            Err(AppError::NotFound)
        }
        async fn find_for_member(
            &self,
            _: UserId,
            _: ConversationId,
        ) -> Result<Conversation, AppError> {
            Err(AppError::NotFound)
        }
        async fn members_for_member(
            &self,
            _: UserId,
            _: ConversationId,
        ) -> Result<Vec<User>, AppError> {
            Err(AppError::NotFound)
        }
        async fn members_presence_for_member(
            &self,
            _: UserId,
            _: ConversationId,
        ) -> Result<Vec<(User, bool)>, AppError> {
            Err(AppError::NotFound)
        }
        async fn share_conversation(&self, _: UserId, _: UserId) -> Result<bool, AppError> {
            Ok(false)
        }
    }

    fn user(name: &str) -> User {
        User {
            id: UserId::new(),
            username: Username::parse(name).unwrap(),
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn direct_creation_normalizes_username_and_rejects_self_conversations() {
        let alice = user("alice");
        let bobby = user("bobby");
        let users = Arc::new(Users(vec![alice.clone(), bobby.clone()]));
        let conversations = Arc::new(Conversations::default());
        let service = ConversationService::new(
            users,
            conversations.clone(),
            ConversationConfig {
                max_members: 100,
                max_page_size: 100,
            },
        );

        let created = service.create_direct(alice.id, "BOBBY").await.unwrap();
        assert_eq!(created.members, vec![alice.id, bobby.id]);
        assert_eq!(
            conversations.0.lock().unwrap().as_slice(),
            &[(alice.id, bobby.id)]
        );
        assert!(matches!(
            service.create_direct(alice.id, "Alice").await,
            Err(AppError::InvalidConversationRequest)
        ));
        assert_eq!(conversations.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn group_creation_enforces_cap_including_creator() {
        let service = ConversationService::new(
            Arc::new(Users::default()),
            Arc::new(Conversations::default()),
            ConversationConfig {
                max_members: 2,
                max_page_size: 100,
            },
        );
        let caller = UserId::new();
        let too_many = service
            .create_group(caller, &["bobby".into(), "carol".into()])
            .await;
        assert!(matches!(too_many, Err(AppError::ConversationMemberLimit)));
    }
}

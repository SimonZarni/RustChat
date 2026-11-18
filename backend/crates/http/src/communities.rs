//! Authenticated community, text-channel and role endpoints.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rustchat_domain::{
    community::{Category, Channel, Community, Role},
    identity::{CategoryId, ChannelId, CommunityId, InviteId, RoleId, User, UserId},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState, CommunityEvent,
    auth::{ApiJson, AuthUser},
};

fn publish_community_event(state: &AppState, community_id: CommunityId, kind: &'static str) {
    state.publish_community(CommunityEvent {
        community_id: community_id.as_uuid(),
        kind,
        occurred_at: time::OffsetDateTime::now_utc(),
    });
}

async fn publish_community_notification(
    state: &AppState,
    community_id: CommunityId,
    actor: UserId,
    event_type: &'static str,
) -> Result<(), ApiError> {
    if let Some(service) = state.notifications.as_deref() {
        for (recipient, source) in service
            .community_event(community_id, actor, event_type)
            .await?
        {
            state.publish_notification(crate::NotificationEvent {
                event_id: source,
                recipient_id: recipient.as_uuid(),
                kind: "community_event".to_owned(),
                occurred_at: time::OffsetDateTime::now_utc(),
            });
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCommunity {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCommunity {
    name: String,
    description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferOwnership {
    new_owner_id: Uuid,
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmAction {
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinCommunity {
    invite_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInvite {
    ttl_seconds: Option<i64>,
    max_uses: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateChannel {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    read_only: bool,
    #[serde(default)]
    category_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateChannel {
    name: String,
    description: String,
    read_only: bool,
    position: i32,
    #[serde(default)]
    category_id: Option<Option<Uuid>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCategory {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCategory {
    name: String,
    position: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRole {
    name: String,
    permissions: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRole {
    name: String,
    permissions: u64,
    position: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionOverride {
    allow: u64,
    deny: u64,
}

#[derive(Serialize)]
pub(super) struct CommunityDto {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    description: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    member_count: usize,
}

#[derive(Serialize)]
pub(super) struct InviteDto {
    id: Uuid,
    community_id: Uuid,
    created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    expires_at: Option<time::OffsetDateTime>,
    max_uses: Option<i32>,
    use_count: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    revoked_at: Option<time::OffsetDateTime>,
}

#[derive(Serialize)]
pub(super) struct InvitesEnvelope {
    invites: Vec<InviteDto>,
}
impl From<Community> for CommunityDto {
    fn from(value: Community) -> Self {
        Self {
            id: value.id.as_uuid(),
            owner_id: value.owner_id.as_uuid(),
            name: value.name,
            description: value.description,
            created_at: value.created_at,
            member_count: value.member_count,
        }
    }
}

#[derive(Serialize)]
pub(super) struct ChannelDto {
    id: Uuid,
    community_id: Uuid,
    conversation_id: Uuid,
    category_id: Option<Uuid>,
    name: String,
    description: String,
    position: i32,
    read_only: bool,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}
impl From<Channel> for ChannelDto {
    fn from(value: Channel) -> Self {
        Self {
            id: value.id.as_uuid(),
            community_id: value.community_id.as_uuid(),
            conversation_id: value.conversation_id.as_uuid(),
            category_id: value.category_id.map(|id| id.as_uuid()),
            name: value.name,
            description: value.description,
            position: value.position,
            read_only: value.read_only,
            created_at: value.created_at,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CategoryDto {
    id: Uuid,
    community_id: Uuid,
    name: String,
    position: i32,
}
impl From<Category> for CategoryDto {
    fn from(value: Category) -> Self {
        Self {
            id: value.id.as_uuid(),
            community_id: value.community_id.as_uuid(),
            name: value.name,
            position: value.position,
        }
    }
}
#[derive(Serialize)]
pub(super) struct CategoriesEnvelope {
    categories: Vec<CategoryDto>,
}
#[derive(Serialize)]
pub(super) struct CategoryEnvelope {
    category: CategoryDto,
}

#[derive(Serialize)]
pub(super) struct RoleDto {
    id: Uuid,
    community_id: Uuid,
    name: String,
    position: i32,
    permissions: u64,
    is_default: bool,
}
impl From<Role> for RoleDto {
    fn from(value: Role) -> Self {
        Self {
            id: value.id.as_uuid(),
            community_id: value.community_id.as_uuid(),
            name: value.name,
            position: value.position,
            permissions: value.permissions,
            is_default: value.is_default,
        }
    }
}

#[derive(Serialize)]
pub(super) struct UserDto {
    id: Uuid,
    username: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}
impl From<User> for UserDto {
    fn from(value: User) -> Self {
        Self {
            id: value.id.as_uuid(),
            username: value.username.to_string(),
            created_at: value.created_at,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CommunityEnvelope {
    community: CommunityDto,
}
#[derive(Serialize)]
pub(super) struct CommunitiesEnvelope {
    communities: Vec<CommunityDto>,
}
#[derive(Serialize)]
pub(super) struct MembersEnvelope {
    members: Vec<UserDto>,
}
#[derive(Serialize)]
pub(super) struct ChannelsEnvelope {
    channels: Vec<ChannelDto>,
}
#[derive(Serialize)]
pub(super) struct RolesEnvelope {
    roles: Vec<RoleDto>,
}
#[derive(Serialize)]
pub(super) struct InviteEnvelope {
    invite_code: String,
}

pub async fn create(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<CreateCommunity>,
) -> Result<(StatusCode, Json<CommunityEnvelope>), ApiError> {
    let community = state
        .communities
        .create(auth.user.id, &body.name, &body.description)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CommunityEnvelope {
            community: community.into(),
        }),
    ))
}

pub async fn list(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<CommunitiesEnvelope>, ApiError> {
    let communities = state
        .communities
        .list(auth.user.id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(CommunitiesEnvelope { communities }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<UpdateCommunity>,
) -> Result<Json<CommunityEnvelope>, ApiError> {
    let community = state
        .communities
        .update_settings(
            auth.user.id,
            community_id(&raw)?,
            &body.name,
            &body.description,
        )
        .await?;
    Ok(Json(CommunityEnvelope {
        community: community.into(),
    }))
}

pub async fn transfer_ownership(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<TransferOwnership>,
) -> Result<StatusCode, ApiError> {
    state
        .communities
        .transfer_ownership(
            auth.user.id,
            community_id(&raw)?,
            UserId::from_uuid(body.new_owner_id),
            body.confirm,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_community(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<ConfirmAction>,
) -> Result<StatusCode, ApiError> {
    state
        .communities
        .delete_community(auth.user.id, community_id(&raw)?, body.confirm)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn members(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<MembersEnvelope>, ApiError> {
    let members = state
        .communities
        .members(auth.user.id, community_id(&raw)?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(MembersEnvelope { members }))
}

pub async fn leave(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<StatusCode, ApiError> {
    let community = community_id(&raw)?;
    state.communities.leave(auth.user.id, community).await?;
    publish_community_notification(&state, community, auth.user.id, "member_left").await?;
    publish_community_event(&state, community, "members.updated");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((raw_community, raw_user)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let member = Uuid::parse_str(&raw_user)
        .map(UserId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community = community_id(&raw_community)?;
    state
        .communities
        .remove_member(auth.user.id, community, member)
        .await?;
    publish_community_notification(&state, community, auth.user.id, "member_removed").await?;
    publish_community_event(&state, community, "members.updated");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_invite(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<CreateInvite>,
) -> Result<(StatusCode, Json<InviteEnvelope>), ApiError> {
    let code = state
        .communities
        .create_invite(
            auth.user.id,
            community_id(&raw)?,
            body.ttl_seconds,
            body.max_uses,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(InviteEnvelope { invite_code: code }),
    ))
}

pub async fn audit_log(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    axum::extract::Query(query): axum::extract::Query<crate::admin::AuditQuery>,
) -> Result<Json<crate::admin::AuditEnvelope>, ApiError> {
    let (before, limit) = query.parse()?;
    let page = state
        .communities
        .list_audit(auth.user.id, community_id(&raw)?, before, limit)
        .await?;
    Ok(Json(page.into()))
}

pub async fn list_invites(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<InvitesEnvelope>, ApiError> {
    let invites = state
        .communities
        .list_invites(auth.user.id, community_id(&raw)?)
        .await?;
    Ok(Json(InvitesEnvelope {
        invites: invites
            .into_iter()
            .map(|invite| InviteDto {
                id: invite.id.as_uuid(),
                community_id: invite.community_id.as_uuid(),
                created_by: invite.created_by.as_uuid(),
                created_at: invite.created_at,
                expires_at: invite.expires_at,
                max_uses: invite.max_uses,
                use_count: invite.use_count,
                revoked_at: invite.revoked_at,
            })
            .collect(),
    }))
}

pub async fn revoke_invite_by_id(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((raw, invite)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let invite = Uuid::parse_str(&invite)
        .map(InviteId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community = community_id(&raw)?;
    state
        .communities
        .revoke_invite_by_id(auth.user.id, community, invite)
        .await?;
    publish_community_event(&state, community, "invites.updated");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeInvite {
    invite_code: String,
}

pub async fn revoke_invite(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<RevokeInvite>,
) -> Result<StatusCode, ApiError> {
    state
        .communities
        .revoke_invite(auth.user.id, community_id(&raw)?, &body.invite_code)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn join(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<JoinCommunity>,
) -> Result<(StatusCode, Json<CommunityEnvelope>), ApiError> {
    let community = state
        .communities
        .join(auth.user.id, &body.invite_code)
        .await?;
    publish_community_notification(&state, community.id, auth.user.id, "member_joined").await?;
    publish_community_event(&state, community.id, "members.updated");
    Ok((
        StatusCode::OK,
        Json(CommunityEnvelope {
            community: community.into(),
        }),
    ))
}

pub async fn channels(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<ChannelsEnvelope>, ApiError> {
    let channels = state
        .communities
        .channels(auth.user.id, community_id(&raw)?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ChannelsEnvelope { channels }))
}

pub async fn categories(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<CategoriesEnvelope>, ApiError> {
    let categories = state
        .communities
        .categories(auth.user.id, community_id(&raw)?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(CategoriesEnvelope { categories }))
}

pub async fn create_category(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<CreateCategory>,
) -> Result<(StatusCode, Json<CategoryEnvelope>), ApiError> {
    let community = community_id(&raw)?;
    let category = state
        .communities
        .create_category(auth.user.id, community, &body.name)
        .await?;
    publish_community_event(&state, community, "channels.updated");
    Ok((
        StatusCode::CREATED,
        Json(CategoryEnvelope {
            category: category.into(),
        }),
    ))
}

pub async fn update_category(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((_, raw)): Path<(String, String)>,
    ApiJson(body): ApiJson<UpdateCategory>,
) -> Result<Json<CategoryEnvelope>, ApiError> {
    let id = Uuid::parse_str(&raw)
        .map(CategoryId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let category = state
        .communities
        .update_category(auth.user.id, id, &body.name, body.position)
        .await?;
    publish_community_event(&state, category.community_id, "channels.updated");
    Ok(Json(CategoryEnvelope {
        category: category.into(),
    }))
}

pub async fn delete_category(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((raw_community, raw)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let id = Uuid::parse_str(&raw)
        .map(CategoryId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community = community_id(&raw_community)?;
    state.communities.delete_category(auth.user.id, id).await?;
    publish_community_event(&state, community, "channels.updated");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_channel(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<CreateChannel>,
) -> Result<(StatusCode, Json<ChannelEnvelope>), ApiError> {
    let channel = state
        .communities
        .create_channel(
            auth.user.id,
            community_id(&raw)?,
            &body.name,
            &body.description,
            body.category_id.map(CategoryId::from_uuid),
            body.read_only,
        )
        .await?;
    publish_community_notification(
        &state,
        channel.community_id,
        auth.user.id,
        "channel_created",
    )
    .await?;
    publish_community_event(&state, channel.community_id, "channels.updated");
    Ok((
        StatusCode::CREATED,
        Json(ChannelEnvelope {
            channel: channel.into(),
        }),
    ))
}

pub async fn update_channel(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((_, raw)): Path<(String, String)>,
    ApiJson(body): ApiJson<UpdateChannel>,
) -> Result<Json<ChannelEnvelope>, ApiError> {
    let id = Uuid::parse_str(&raw)
        .map(ChannelId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let channel = state
        .communities
        .update_channel(
            auth.user.id,
            id,
            &body.name,
            &body.description,
            body.category_id.map(|id| id.map(CategoryId::from_uuid)),
            body.read_only,
            body.position,
        )
        .await?;
    publish_community_notification(
        &state,
        channel.community_id,
        auth.user.id,
        "channel_updated",
    )
    .await?;
    publish_community_event(&state, channel.community_id, "channels.updated");
    Ok(Json(ChannelEnvelope {
        channel: channel.into(),
    }))
}

pub async fn delete_channel(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((raw_community, raw)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let id = Uuid::parse_str(&raw)
        .map(ChannelId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community = community_id(&raw_community)?;
    state.communities.delete_channel(auth.user.id, id).await?;
    publish_community_notification(&state, community, auth.user.id, "channel_deleted").await?;
    publish_community_event(&state, community, "channels.updated");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub(super) struct ChannelEnvelope {
    channel: ChannelDto,
}

pub async fn roles(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<RolesEnvelope>, ApiError> {
    let roles = state
        .communities
        .roles(auth.user.id, community_id(&raw)?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(RolesEnvelope { roles }))
}

pub async fn create_role(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<CreateRole>,
) -> Result<(StatusCode, Json<RoleEnvelope>), ApiError> {
    let role = state
        .communities
        .create_role(
            auth.user.id,
            community_id(&raw)?,
            &body.name,
            body.permissions,
        )
        .await?;
    publish_community_event(&state, role.community_id, "members.updated");
    Ok((
        StatusCode::CREATED,
        Json(RoleEnvelope { role: role.into() }),
    ))
}

pub async fn update_role(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((community, raw_role)): Path<(String, String)>,
    ApiJson(body): ApiJson<UpdateRole>,
) -> Result<Json<RoleEnvelope>, ApiError> {
    let role = Uuid::parse_str(&raw_role)
        .map(RoleId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let role = state
        .communities
        .update_role(
            auth.user.id,
            community_id(&community)?,
            role,
            &body.name,
            body.permissions,
            body.position,
        )
        .await?;
    publish_community_event(&state, role.community_id, "members.updated");
    Ok(Json(RoleEnvelope { role: role.into() }))
}

pub async fn delete_role(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((community, raw_role)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let role = Uuid::parse_str(&raw_role)
        .map(RoleId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community_id = community_id(&community)?;
    state
        .communities
        .delete_role(auth.user.id, community_id, role)
        .await?;
    publish_community_event(&state, community_id, "members.updated");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub(super) struct RoleEnvelope {
    role: RoleDto,
}

pub async fn assign_role(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((community, user, role)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    let user = Uuid::parse_str(&user)
        .map(UserId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let role = Uuid::parse_str(&role)
        .map(RoleId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community_id = community_id(&community)?;
    state
        .communities
        .assign_role(auth.user.id, community_id, user, role)
        .await?;
    publish_community_event(&state, community_id, "members.updated");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_role(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((community, user, role)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    let user = Uuid::parse_str(&user)
        .map(UserId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let role = Uuid::parse_str(&role)
        .map(RoleId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community_id = community_id(&community)?;
    state
        .communities
        .remove_role(auth.user.id, community_id, user, role)
        .await?;
    publish_community_event(&state, community_id, "members.updated");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_channel_override(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((channel, role)): Path<(String, String)>,
    ApiJson(body): ApiJson<PermissionOverride>,
) -> Result<StatusCode, ApiError> {
    let channel = Uuid::parse_str(&channel)
        .map(ChannelId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let role = Uuid::parse_str(&role)
        .map(RoleId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let community = state
        .communities
        .set_channel_override(auth.user.id, channel, role, body.allow, body.deny)
        .await?;
    publish_community_event(&state, community, "channels.updated");
    Ok(StatusCode::NO_CONTENT)
}

fn community_id(raw: &str) -> Result<CommunityId, ApiError> {
    Uuid::parse_str(raw)
        .map(CommunityId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))
}

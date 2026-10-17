//! OpenAPI document root: info, security scheme and tags.

use utoipa::openapi::OpenApi as OpenApiDoc;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};

pub const TAG_SYSTEM: &str = "system";
pub const TAG_AUTH: &str = "auth";
pub const TAG_DIRECTORY: &str = "directory";
pub const TAG_GROUP_ORDERS: &str = "group-orders";
pub const TAG_PUBLIC: &str = "public";
pub const TAG_AI_IMPORT: &str = "ai-import";

#[derive(OpenApi)]
#[openapi(
    info(title = "order-a-bento API", version = "0.1.0"),
    modifiers(&SecurityAddon),
    tags(
        (name = TAG_SYSTEM, description = "Health and operational endpoints"),
        (name = TAG_AUTH, description = "Login, logout and the current user"),
        (name = TAG_DIRECTORY, description = "Rosters, vendors and menus (master data)"),
        (name = TAG_GROUP_ORDERS, description = "Opening and viewing group orders (organizer)"),
        (name = TAG_PUBLIC, description = "Share-token ordering flow for participants (no login)"),
        (name = TAG_AI_IMPORT, description = "Import rosters and vendor menus from a photo with AI (organizer)")
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut OpenApiDoc) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "cookieAuth",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("sid"))),
        );
    }
}

impl ApiDoc {
    /// The base document that routes are merged into.
    pub fn base() -> OpenApiDoc {
        <Self as OpenApi>::openapi()
    }
}

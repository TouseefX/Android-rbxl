//! Canonical Roblox web/CDN subdomains and small URL builders.
//!
//! This is just a reference table — the actual HTTP calls live in
//! `roblox_api::WebClient`. Keeping the hosts in one place makes it easier
//! to spot when an endpoint moves and avoids scattering string literals
//! across the codebase.

/// All current Roblox subdomains (as of the last update). The ones most
/// relevant to an editor are marked with the kind of traffic they carry.
pub const SUBDOMAINS: &[&str] = &[
    "apis.roblox.com",                // Open Cloud
    "accountinformation.roblox.com",
    "accountsettings.roblox.com",
    "adconfiguration.roblox.com",
    "assetdelivery.roblox.com",      // asset/place/mesh downloads
    "avatar.roblox.com",
    "badges.roblox.com",
    "catalog.roblox.com",
    "clientsettings.roblox.com",
    "contacts.roblox.com",
    "develop.roblox.com",            // universe/place configuration
    "economy.roblox.com",
    "economycreatorstats.roblox.com",
    "engagementpayouts.roblox.com",
    "followings.roblox.com",
    "friends.roblox.com",
    "gameinternationalization.roblox.com",
    "games.roblox.com",
    "groups.roblox.com",
    "inventory.roblox.com",
    "itemconfiguration.roblox.com",  // tags, catalog metadata
    "locale.roblox.com",
    "localizationtables.roblox.com",
    "notifications.roblox.com",
    "premiumfeatures.roblox.com",
    "presence.roblox.com",
    "privatemessages.roblox.com",
    "publish.roblox.com",            // publishing hostname (see note)
    "thumbnails.roblox.com",
    "thumbnailsresizer.roblox.com",
    "trades.roblox.com",
    "translationroles.roblox.com",
    "twostepverification.roblox.com",
    "users.roblox.com",
];

// Note on publishing: as of 2025 the legacy assetgame.roblox.com/Asset/
// .ashx gateway is retired for third-party uploads. The supported path is
// Open Cloud:
//   POST https://apis.roblox.com/universes/v1/{universeId}/places/{placeId}/versions?versionType=Published|Saved
// with header `x-api-key: <open-cloud-key>` and an octet-stream body.

pub const OPEN_CLOUD_BASE: &str = "https://apis.roblox.com";
pub const ASSET_DELIVERY: &str = "https://assetdelivery.roblox.com";
pub const DEVELOP: &str = "https://develop.roblox.com";
pub const THUMBNAILS: &str = "https://thumbnails.roblox.com";
pub const USERS: &str = "https://users.roblox.com";
pub const AVATAR: &str = "https://avatar.roblox.com";
pub const INVENTORY: &str = "https://inventory.roblox.com";
pub const BADGES: &str = "https://badges.roblox.com";
pub const GAMES: &str = "https://games.roblox.com";
pub const GROUPS: &str = "https://groups.roblox.com";
pub const ECONOMY: &str = "https://economy.roblox.com";
pub const CATALOG: &str = "https://catalog.roblox.com";

/// Build an Open Cloud place-publish URL.
pub fn open_cloud_publish_url(universe_id: u64, place_id: u64, published: bool) -> String {
    let version_type = if published { "Published" } else { "Saved" };
    format!(
        "{OPEN_CLOUD_BASE}/universes/v1/{universe_id}/places/{place_id}/versions?versionType={version_type}"
    )
}

/// Build the asset-delivery URL for a place/asset/mesh ID.
pub fn asset_delivery_url(asset_id: u64) -> String {
    format!("{ASSET_DELIVERY}/v1/asset/?id={asset_id}")
}

/// Build a thumbnail-request URL for a batch of asset IDs (the batch
/// endpoint returns URLs to the actual rendered images).
pub fn thumbnails_batch_url() -> String {
    format!("{THUMBNAILS}/v1/assets/batch")
}

/// Build the "start-server" / game-join URL for documentation purposes.
/// (This orchestrates game-client joins, not Studio editing.)
pub fn gamejoin_url(place_id: u64, universe_id: u64) -> String {
    format!(
        "https://gamejoin.roblox.com/v1/join-game?placeId={place_id}&universeId={universe_id}"
    )
}

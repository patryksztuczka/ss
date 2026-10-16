//! Silent system notifications for failures. `UNUserNotificationCenter`
//! crashes in a process without a bundle, so unbundled runs only log.

use std::sync::Once;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNUserNotificationCenter,
};

pub fn notify(title: &str, body: &str) {
    tracing::info!(%title, %body, "notification");
    if !crate::system::is_bundled() {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    static AUTHORIZE: Once = Once::new();
    AUTHORIZE.call_once(|| {
        let done = RcBlock::new(|granted: Bool, _error: *mut NSError| {
            if !granted.as_bool() {
                tracing::info!("notifications are not allowed");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &done,
        );
    });
    // No sound: Electron posted these with `silent: true`.
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let identifier = NSString::from_str(&crate::capture_store::new_id());
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None);
    center.addNotificationRequest_withCompletionHandler(&request, None);
}

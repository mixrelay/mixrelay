//! Paid access for Driver requests. On macOS the service runs without the signed-in
//! user's keychain, so the signed `MixRelay` app checks the license and claims paid access;
//! the claim counts only from a connected process that is that app, signed by this
//! service's own team.
//!
//! Threat model: this keeps the paid gate working for the user's own app. It does not
//! resist a determined local user, who can already edit the Driver's user-owned settings
//! file; against such a user a Unix-socket peer token is also only as good as the process
//! that holds the socket (a connector may exec into the app before its token is read).
//! Only the app is trusted, not the whole team: the engine disables library validation to
//! host plug-ins, so its identity says nothing about the code that sends a claim.

use mixrelay_driver_control::AccessRequest;

/// Accepts a claim only from a peer `verify` vouches for (by its audit token).
pub(crate) fn decide(request: &AccessRequest, verify: impl Fn(&[u8; 32]) -> bool) -> bool {
    request.claimed
        && request
            .peer
            .and_then(|peer| peer.audit_token)
            .is_some_and(|token| verify(&token))
}

#[cfg(target_os = "macos")]
pub(crate) fn policy(request: &AccessRequest) -> bool {
    decide(request, |token| {
        // Unsigned development services trust local claims; signed releases never do.
        signing::own_team().map_or(cfg!(debug_assertions), |team| {
            signing::signed_by_team(token, team)
        })
    })
}

#[cfg(target_os = "macos")]
mod signing {
    use core_foundation::{
        base::{CFType, TCFType as _},
        data::CFData,
        dictionary::{CFDictionary, CFDictionaryRef},
        string::{CFString, CFStringRef},
    };
    use std::{ffi::c_void, sync::OnceLock};

    /// The signed app (editor and runtime are one executable).
    const APP_IDENTIFIER: &str = "io.mixrelay.desktop";

    type SecCode = *mut c_void;
    type SecRequirement = *mut c_void;

    const SIGNING_INFORMATION: u32 = 1 << 1;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecGuestAttributeAudit: CFStringRef;
        static kSecCodeInfoTeamIdentifier: CFStringRef;
        fn SecCodeCopySelf(flags: u32, code: *mut SecCode) -> i32;
        fn SecCodeCopySigningInformation(
            code: SecCode,
            flags: u32,
            information: *mut CFDictionaryRef,
        ) -> i32;
        fn SecCodeCopyGuestWithAttributes(
            host: SecCode,
            attributes: CFDictionaryRef,
            flags: u32,
            guest: *mut SecCode,
        ) -> i32;
        fn SecRequirementCreateWithString(
            text: CFStringRef,
            flags: u32,
            requirement: *mut SecRequirement,
        ) -> i32;
        fn SecCodeCheckValidity(code: SecCode, flags: u32, requirement: SecRequirement) -> i32;
        fn CFRelease(object: *const c_void);
    }

    /// The team that signed this service, once; `None` when unsigned or ad hoc.
    pub(super) fn own_team() -> Option<&'static str> {
        static TEAM: OnceLock<Option<String>> = OnceLock::new();
        TEAM.get_or_init(read_own_team).as_deref()
    }

    fn read_own_team() -> Option<String> {
        let mut code: SecCode = std::ptr::null_mut();
        // SAFETY: out-parameters are written only on success and released below.
        if unsafe { SecCodeCopySelf(0, &raw mut code) } != 0 || code.is_null() {
            return None;
        }
        let mut information: CFDictionaryRef = std::ptr::null();
        // SAFETY: `code` is a live SecCode owned by this function.
        let status = unsafe {
            SecCodeCopySigningInformation(code, SIGNING_INFORMATION, &raw mut information)
        };
        // SAFETY: created by SecCodeCopySelf above.
        unsafe { CFRelease(code.cast_const()) };
        if status != 0 || information.is_null() {
            return None;
        }
        // SAFETY: a +1 dictionary returned by the copy function above.
        let information: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_create_rule(information) };
        // SAFETY: a constant key exported by Security.framework.
        let key = unsafe { CFString::wrap_under_get_rule(kSecCodeInfoTeamIdentifier) };
        information
            .find(&key)
            .and_then(|team| team.downcast::<CFString>())
            .map(|team| team.to_string())
            .filter(|team| {
                team.len() == 10 && team.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
    }

    /// Whether the process named by `token` is validly signed by `team`.
    pub(super) fn signed_by_team(token: &[u8; 32], team: &str) -> bool {
        let token = CFData::from_buffer(token);
        // SAFETY: a constant key exported by Security.framework.
        let key = unsafe { CFString::wrap_under_get_rule(kSecGuestAttributeAudit) };
        let attributes = CFDictionary::from_CFType_pairs(&[(key, token.as_CFType())]);
        let mut guest: SecCode = std::ptr::null_mut();
        // SAFETY: valid dictionary; the out-parameter is released below when set.
        let status = unsafe {
            SecCodeCopyGuestWithAttributes(
                std::ptr::null_mut(),
                attributes.as_concrete_TypeRef(),
                0,
                &raw mut guest,
            )
        };
        if status != 0 || guest.is_null() {
            return false;
        }
        let text = CFString::new(&format!(
            "identifier \"{APP_IDENTIFIER}\" and anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
        ));
        let mut requirement: SecRequirement = std::ptr::null_mut();
        // SAFETY: valid string; the out-parameter is released below when set.
        let valid = unsafe {
            SecRequirementCreateWithString(text.as_concrete_TypeRef(), 0, &raw mut requirement)
        } == 0
            && !requirement.is_null()
            // SAFETY: both objects are live until released below.
            && unsafe { SecCodeCheckValidity(guest, 0, requirement) } == 0;
        // SAFETY: each object was created above and is released exactly once.
        unsafe {
            if !requirement.is_null() {
                CFRelease(requirement.cast_const());
            }
            CFRelease(guest.cast_const());
        }
        valid
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[allow(
            deprecated,
            reason = "libc's Mach bindings suffice for this test-only query"
        )]
        fn own_token() -> [u8; 32] {
            // <mach/task_info.h>: TASK_AUDIT_TOKEN, eight natural words.
            let mut token = [0_u8; 32];
            let mut count: libc::mach_msg_type_number_t = 8;
            // SAFETY: a 32-byte buffer for TASK_AUDIT_TOKEN of this task.
            let result = unsafe {
                libc::task_info(
                    libc::mach_task_self(),
                    15,
                    token.as_mut_ptr().cast(),
                    &raw mut count,
                )
            };
            assert_eq!(result, 0);
            token
        }

        #[test]
        fn an_unsigned_or_mismatched_process_is_not_vouched_for() {
            // Test binaries are ad-hoc signed: no team, so no release would trust them.
            assert_eq!(own_team(), None);
            assert!(!signed_by_team(&own_token(), "38UUAD7TUP"));
            assert!(!signed_by_team(&[0; 32], "38UUAD7TUP"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_local_socket::Peer;

    #[test]
    fn only_a_vouched_peer_can_claim_paid_access() {
        let peer = Peer {
            uid: 501,
            audit_token: Some([1; 32]),
        };
        let trusted = |token: &[u8; 32]| token == &[1; 32];
        for (claimed, peer, expected) in [
            (true, Some(peer), true),
            (false, Some(peer), false),
            (true, None, false),
            (
                true,
                Some(Peer {
                    audit_token: None,
                    ..peer
                }),
                false,
            ),
            (
                true,
                Some(Peer {
                    audit_token: Some([2; 32]),
                    ..peer
                }),
                false,
            ),
        ] {
            assert_eq!(decide(&AccessRequest { claimed, peer }, trusted), expected);
        }
    }
}

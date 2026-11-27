use crate::models::Account;
use crate::modules::{db, device, process, version};
use std::collections::HashMap;
use std::fs;
// Command 仅用于 macos/Linux 分支（security / secret-tool / kill），Windows 裁剪不导入以免 unused
#[cfg(not(windows))]
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// System Keychain/Secret-Service passive read cache & denial-suppression coordinator.
///
/// Kept cross-platform so its state-machine and failure-classification logic are unit-tested
/// across Ubuntu/Windows/macOS CI while macOS and Linux use it to serialize credential
/// lookups (`security find-generic-password` / `secret-tool lookup`), avoid concurrent Keychain
/// password dialog storms, and suppress passive re-probes once the user cancels or denies
/// a prompt until an explicit write/switch calls `invalidate()`. Active writes on macOS use
/// native `Security.framework` FFI (`SecItemAdd` with unrestricted `SecAccessRef`) so credentials
/// never appear in process `argv` and aren't limited by `security -i`'s 4KB line buffer.
#[allow(dead_code)]
const KEYCHAIN_PASSIVE_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum KeychainLookupOutcome {
    Found(String),
    NotFound,
    Denied(String),
}

#[allow(dead_code)]
struct KeychainCacheSlot {
    outcome: Option<(KeychainLookupOutcome, Instant)>,
}

#[allow(dead_code)]
type KeychainSlotMap = HashMap<(String, String), Arc<Mutex<KeychainCacheSlot>>>;

#[allow(dead_code)]
pub(crate) struct KeychainReadCoordinator {
    slots: Mutex<KeychainSlotMap>,
    success_ttl: Duration,
    /// `None` suppresses passive retries after `Denied` indefinitely until `invalidate()`.
    denied_cooldown: Option<Duration>,
}

#[allow(dead_code)]
impl KeychainReadCoordinator {
    fn new(success_ttl: Duration, denied_cooldown: Option<Duration>) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            success_ttl,
            denied_cooldown,
        }
    }

    fn slot_for(&self, service: &str, account: &str) -> Arc<Mutex<KeychainCacheSlot>> {
        let mut map = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        map.entry((service.to_string(), account.to_string()))
            .or_insert_with(|| Arc::new(Mutex::new(KeychainCacheSlot { outcome: None })))
            .clone()
    }

    /// Execute a single-flight Keychain lookup for `(service, account)`.
    /// Concurrent readers for the same item serialize on the per-item mutex; when `bypass_cache`
    /// is false, recent `Found`/`NotFound` results (60s) and `Denied` results (sticky until
    /// `invalidate()`) are served without re-invoking external keychain tools.
    pub(crate) fn read_with<F>(
        &self,
        service: &str,
        account: &str,
        bypass_cache: bool,
        fetch: F,
    ) -> KeychainLookupOutcome
    where
        F: FnOnce() -> KeychainLookupOutcome,
    {
        let slot = self.slot_for(service, account);
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());

        if !bypass_cache {
            if let Some((ref cached, at)) = guard.outcome {
                let still_valid = match cached {
                    KeychainLookupOutcome::Denied(_) => match self.denied_cooldown {
                        Some(ttl) => at.elapsed() < ttl,
                        None => true,
                    },
                    KeychainLookupOutcome::Found(_) | KeychainLookupOutcome::NotFound => {
                        at.elapsed() < self.success_ttl
                    }
                };
                if still_valid {
                    return cached.clone();
                }
            }
        }

        let outcome = fetch();
        guard.outcome = Some((outcome.clone(), Instant::now()));
        outcome
    }

    pub(crate) fn invalidate(&self, service: &str, account: &str) {
        let slot = self.slot_for(service, account);
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        guard.outcome = None;
    }
}

#[allow(dead_code)]
pub(crate) fn classify_macos_security_failure(
    exit_code: Option<i32>,
    stderr: &str,
) -> KeychainLookupOutcome {
    let trimmed = stderr.trim();
    let lower = trimmed.to_lowercase();

    // errSecItemNotFound (44) means the entry genuinely does not exist (no user prompt shown).
    if exit_code == Some(44) || lower.contains("could not be found") {
        return KeychainLookupOutcome::NotFound;
    }

    // errSecInteractionNotAllowed (36), errSecAuthFailed (51), errSecUserCanceled (128),
    // or explicit macOS Keychain denial/cancellation diagnostics -> back off via cooldown.
    if matches!(exit_code, Some(36 | 51 | 128))
        || lower.contains("user interaction is not allowed")
        || lower.contains("user canceled")
        || lower.contains("user cancelled")
        || lower.contains("canceled")
        || lower.contains("cancelled")
        || lower.contains("denied")
        || lower.contains("authentication failed")
        || lower.contains("authfailed")
        || lower.contains("errsecusercanceled")
        || lower.contains("errsecinteractionnotallowed")
    {
        let reason = if trimmed.is_empty() {
            format!("macOS Keychain access denied (exit {:?})", exit_code)
        } else {
            trimmed.to_string()
        };
        return KeychainLookupOutcome::Denied(reason);
    }

    KeychainLookupOutcome::NotFound
}

#[allow(dead_code)]
pub(crate) fn classify_linux_secret_tool_failure(
    exit_code: Option<i32>,
    stderr: &str,
) -> KeychainLookupOutcome {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        return KeychainLookupOutcome::NotFound;
    }

    let lower = trimmed.to_lowercase();
    if lower.contains("not found") || lower.contains("no matching item") {
        return KeychainLookupOutcome::NotFound;
    }

    // Only back off with Denied when secret-tool reports an explicit permission/unlock/prompt
    // rejection or Secret Service / D-Bus failure, not benign GTK/GLib/dbind warnings.
    if lower.contains("denied")
        || lower.contains("locked")
        || lower.contains("unlock")
        || lower.contains("canceled")
        || lower.contains("cancelled")
        || lower.contains("dismissed")
        || lower.contains("authentication failed")
        || lower.contains("permission")
        || lower.contains("unauthorized")
        || lower.contains("not allowed")
        || lower.contains("org.freedesktop.secrets")
        || lower.contains("secret service")
        || lower.contains("d-bus")
        || lower.contains("dbus")
        || lower.contains("cannot autolaunch")
        || lower.contains("daemon")
    {
        return KeychainLookupOutcome::Denied(trimmed.to_string());
    }

    let _ = exit_code;
    KeychainLookupOutcome::NotFound
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn system_keychain_coordinator() -> &'static KeychainReadCoordinator {
    static COORDINATOR: OnceLock<KeychainReadCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(|| KeychainReadCoordinator::new(KEYCHAIN_PASSIVE_CACHE_TTL, None))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn invalidate_system_keychain_cache(service: &str, account: &str) {
    system_keychain_coordinator().invalidate(service, account);
}

#[cfg(target_os = "macos")]
mod macos_keychain_ffi {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFDataRef = *const c_void;
    type CFBooleanRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type SecAccessRef = *const c_void;
    type SecACLRef = *const c_void;
    type CFIndex = isize;
    type CFStringEncoding = u32;
    type Boolean = u8;
    type OSStatus = i32;
    type SecKeychainPromptSelector = u16;

    const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
    const ERR_SEC_SUCCESS: OSStatus = 0;
    const ERR_SEC_USER_CANCELED: OSStatus = -128;
    const ERR_SEC_AUTH_FAILED: OSStatus = -25293;
    const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
    const ERR_SEC_INTERACTION_NOT_ALLOWED: OSStatus = -25308;
    pub(super) const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;

    pub(super) struct CfGuard(CFTypeRef);

    impl CfGuard {
        unsafe fn from_create(ptr: CFTypeRef) -> Option<Self> {
            if ptr.is_null() {
                None
            } else {
                Some(Self(ptr))
            }
        }

        fn as_ptr(&self) -> CFTypeRef {
            self.0
        }
    }

    impl Drop for CfGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        static kCFTypeArrayCallBacks: c_void;
        static kCFBooleanTrue: CFBooleanRef;

        fn CFRelease(cf: CFTypeRef);
        fn CFStringCreateWithBytes(
            alloc: CFAllocatorRef,
            bytes: *const u8,
            num_bytes: CFIndex,
            encoding: CFStringEncoding,
            is_external_representation: Boolean,
        ) -> CFStringRef;
        fn CFDataCreate(alloc: CFAllocatorRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
        fn CFDataGetLength(the_data: CFDataRef) -> CFIndex;
        fn CFDataGetBytePtr(the_data: CFDataRef) -> *const u8;
        fn CFArrayCreate(
            allocator: CFAllocatorRef,
            values: *const *const c_void,
            num_values: CFIndex,
            callbacks: *const c_void,
        ) -> CFArrayRef;
        fn CFDictionaryCreate(
            allocator: CFAllocatorRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: CFIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;
        fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    }

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecClass: CFStringRef;
        static kSecClassGenericPassword: CFStringRef;
        static kSecAttrService: CFStringRef;
        static kSecAttrAccount: CFStringRef;
        static kSecAttrLabel: CFStringRef;
        static kSecValueData: CFStringRef;
        static kSecAttrAccess: CFStringRef;
        static kSecReturnData: CFStringRef;
        static kSecUseAuthenticationUI: CFStringRef;
        static kSecUseAuthenticationUIFail: CFStringRef;

        fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        fn SecItemDelete(query: CFDictionaryRef) -> OSStatus;
        fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        static kSecACLAuthorizationDecrypt: CFStringRef;
        static kSecACLAuthorizationPartitionID: CFStringRef;
        fn SecAccessCreate(
            descriptor: CFStringRef,
            trustedlist: CFArrayRef,
            access_ref: *mut SecAccessRef,
        ) -> OSStatus;
        fn SecAccessCopyMatchingACLList(
            access_ref: SecAccessRef,
            authorization_tag: CFTypeRef,
        ) -> CFArrayRef;
        fn SecACLCopyContents(
            acl: SecACLRef,
            application_list: *mut CFArrayRef,
            description: *mut CFStringRef,
            prompt_selector: *mut SecKeychainPromptSelector,
        ) -> OSStatus;
        fn SecACLSetContents(
            acl: SecACLRef,
            application_list: CFArrayRef,
            description: CFStringRef,
            prompt_selector: SecKeychainPromptSelector,
        ) -> OSStatus;
        fn SecACLCreateWithSimpleContents(
            access: SecAccessRef,
            application_list: CFArrayRef,
            description: CFStringRef,
            prompt_selector: SecKeychainPromptSelector,
            new_acl: *mut SecACLRef,
        ) -> OSStatus;
        fn SecACLUpdateAuthorizations(acl: SecACLRef, authorizations: CFArrayRef) -> OSStatus;
        fn SecKeychainGetUserInteractionAllowed(state: *mut Boolean) -> OSStatus;
        fn SecKeychainSetUserInteractionAllowed(state: Boolean) -> OSStatus;
    }

    fn make_cf_string(s: &str) -> Result<CfGuard, String> {
        let len = CFIndex::try_from(s.len()).map_err(|_| "Keychain string too long".to_string())?;
        let ptr = unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                s.as_ptr(),
                len,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        };
        unsafe { CfGuard::from_create(ptr) }
            .ok_or_else(|| "CFStringCreateWithBytes failed".to_string())
    }

    fn make_cf_data(bytes: &[u8]) -> Result<CfGuard, String> {
        let len =
            CFIndex::try_from(bytes.len()).map_err(|_| "Keychain payload too long".to_string())?;
        let ptr = unsafe { CFDataCreate(std::ptr::null(), bytes.as_ptr(), len) };
        unsafe { CfGuard::from_create(ptr) }.ok_or_else(|| "CFDataCreate failed".to_string())
    }

    fn make_cf_dict(keys: &[CFTypeRef], values: &[CFTypeRef]) -> Result<CfGuard, String> {
        debug_assert_eq!(keys.len(), values.len());
        let dict = unsafe {
            CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as CFIndex,
                &kCFTypeDictionaryKeyCallBacks as *const c_void,
                &kCFTypeDictionaryValueCallBacks as *const c_void,
            )
        };
        unsafe { CfGuard::from_create(dict) }.ok_or_else(|| "CFDictionaryCreate failed".to_string())
    }

    /// Build a permissive `SecAccessRef` (equivalent to `security add-generic-password -A`)
    /// so external signed binaries (`agy`, `Antigravity.app`, `agy_acp_server`) can read
    /// the credential without triggering macOS Keychain password prompts.
    fn create_unrestricted_access(
        descriptor: &CfGuard,
        include_partition_acl: bool,
    ) -> Result<CfGuard, String> {
        let mut access_raw: SecAccessRef = std::ptr::null();
        let status =
            unsafe { SecAccessCreate(descriptor.as_ptr(), std::ptr::null(), &mut access_raw) };
        if status != ERR_SEC_SUCCESS || access_raw.is_null() {
            return Err(format!("SecAccessCreate failed (OSStatus {})", status));
        }
        let access = unsafe { CfGuard::from_create(access_raw) }
            .ok_or_else(|| "SecAccessCreate returned null".to_string())?;

        // Target only the decrypt ACL so the written Keychain item requires no
        // per-app ACL prompt when Antigravity IDE or helper tools read it later.
        let acl_list_raw: CFArrayRef = unsafe {
            SecAccessCopyMatchingACLList(access.as_ptr(), kSecACLAuthorizationDecrypt as CFTypeRef)
        };
        let acl_list = unsafe { CfGuard::from_create(acl_list_raw) }
            .ok_or_else(|| "SecAccessCopyMatchingACLList returned null".to_string())?;

        let count = unsafe { CFArrayGetCount(acl_list.as_ptr()) };
        let mut updated_count = 0isize;
        for idx in 0..count {
            let acl = unsafe { CFArrayGetValueAtIndex(acl_list.as_ptr(), idx) };
            if acl.is_null() {
                continue;
            }
            let mut app_list_raw: CFArrayRef = std::ptr::null();
            let mut desc_raw: CFStringRef = std::ptr::null();
            let mut prompt_selector: SecKeychainPromptSelector = 0;
            let acl_status = unsafe {
                SecACLCopyContents(acl, &mut app_list_raw, &mut desc_raw, &mut prompt_selector)
            };
            if acl_status != ERR_SEC_SUCCESS {
                return Err(format!(
                    "SecACLCopyContents failed (OSStatus {})",
                    acl_status
                ));
            }
            let _app_list_guard = unsafe { CfGuard::from_create(app_list_raw) };
            let desc_guard = unsafe { CfGuard::from_create(desc_raw) };
            let set_status = unsafe {
                SecACLSetContents(
                    acl,
                    std::ptr::null(),
                    desc_guard
                        .as_ref()
                        .map(|d| d.as_ptr())
                        .unwrap_or(descriptor.as_ptr()),
                    0,
                )
            };
            if set_status != ERR_SEC_SUCCESS {
                return Err(format!(
                    "SecACLSetContents failed (OSStatus {})",
                    set_status
                ));
            }
            updated_count += 1;
        }

        if updated_count == 0 {
            return Err(
                "SecAccessCopyMatchingACLList returned no decrypt ACLs to relax".to_string(),
            );
        }

        if include_partition_acl {
            const PARTITION_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Partitions</key>\n\t<array>\n\t\t<string>apple-tool:</string>\n\t\t<string>apple:</string>\n\t\t<string>unsigned:</string>\n\t\t<string>teamid:EQHXZ8M8AV</string>\n\t</array>\n</dict>\n</plist>\n";
            let mut hex_plist = String::with_capacity(PARTITION_PLIST.len() * 2);
            for b in PARTITION_PLIST.as_bytes() {
                use std::fmt::Write as _;
                let _ = write!(&mut hex_plist, "{:02x}", b);
            }
            let partition_desc = make_cf_string(&hex_plist)?;
            let mut partition_acl_raw: SecACLRef = std::ptr::null();
            let create_acl_status = unsafe {
                SecACLCreateWithSimpleContents(
                    access.as_ptr(),
                    std::ptr::null(),
                    partition_desc.as_ptr(),
                    0,
                    &mut partition_acl_raw,
                )
            };
            if create_acl_status != ERR_SEC_SUCCESS || partition_acl_raw.is_null() {
                return Err(format!(
                    "SecACLCreateWithSimpleContents failed (OSStatus {})",
                    create_acl_status
                ));
            }
            let partition_acl = unsafe { CfGuard::from_create(partition_acl_raw) }
                .ok_or_else(|| "SecACLCreateWithSimpleContents returned null".to_string())?;
            let tag = unsafe { kSecACLAuthorizationPartitionID };
            let auth_tags_raw = unsafe {
                CFArrayCreate(
                    std::ptr::null(),
                    &tag as *const CFStringRef,
                    1,
                    &kCFTypeArrayCallBacks as *const c_void,
                )
            };
            let auth_tags = unsafe { CfGuard::from_create(auth_tags_raw) }
                .ok_or_else(|| "CFArrayCreate returned null".to_string())?;
            let update_status =
                unsafe { SecACLUpdateAuthorizations(partition_acl.as_ptr(), auth_tags.as_ptr()) };
            if update_status != ERR_SEC_SUCCESS {
                return Err(format!(
                    "SecACLUpdateAuthorizations failed (OSStatus {})",
                    update_status
                ));
            }
        }

        Ok(access)
    }

    static USER_INTERACTION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ScopedNoKeychainUi {
        prev_allowed: Boolean,
    }

    impl ScopedNoKeychainUi {
        fn enter() -> Self {
            let mut prev: Boolean = 1;
            unsafe {
                let _ = SecKeychainGetUserInteractionAllowed(&mut prev);
                let _ = SecKeychainSetUserInteractionAllowed(0);
            }
            Self { prev_allowed: prev }
        }
    }

    impl Drop for ScopedNoKeychainUi {
        fn drop(&mut self) {
            unsafe {
                let _ = SecKeychainSetUserInteractionAllowed(self.prev_allowed);
            }
        }
    }

    fn try_add_generic_password(
        service_cf: &CfGuard,
        account_cf: &CfGuard,
        secret_cf: &CfGuard,
        access_cf: Option<&CfGuard>,
    ) -> Result<(), (OSStatus, String)> {
        let (keys, values): (Vec<CFTypeRef>, Vec<CFTypeRef>) = if let Some(access) = access_cf {
            (
                vec![
                    unsafe { kSecClass },
                    unsafe { kSecAttrService },
                    unsafe { kSecAttrAccount },
                    unsafe { kSecAttrLabel },
                    unsafe { kSecValueData },
                    unsafe { kSecAttrAccess },
                ],
                vec![
                    unsafe { kSecClassGenericPassword },
                    service_cf.as_ptr(),
                    account_cf.as_ptr(),
                    service_cf.as_ptr(),
                    secret_cf.as_ptr(),
                    access.as_ptr(),
                ],
            )
        } else {
            (
                vec![
                    unsafe { kSecClass },
                    unsafe { kSecAttrService },
                    unsafe { kSecAttrAccount },
                    unsafe { kSecAttrLabel },
                    unsafe { kSecValueData },
                ],
                vec![
                    unsafe { kSecClassGenericPassword },
                    service_cf.as_ptr(),
                    account_cf.as_ptr(),
                    service_cf.as_ptr(),
                    secret_cf.as_ptr(),
                ],
            )
        };

        let attrs = make_cf_dict(&keys, &values).map_err(|e| (0, e))?;

        let mut status = unsafe { SecItemAdd(attrs.as_ptr(), std::ptr::null_mut()) };
        if status == ERR_SEC_DUPLICATE_ITEM {
            let delete_query = unsafe {
                make_cf_dict(
                    &[kSecClass, kSecAttrService, kSecAttrAccount],
                    &[
                        kSecClassGenericPassword,
                        service_cf.as_ptr(),
                        account_cf.as_ptr(),
                    ],
                )
            };
            if let Ok(dq) = delete_query {
                let _ = unsafe { SecItemDelete(dq.as_ptr()) };
            }
            status = unsafe { SecItemAdd(attrs.as_ptr(), std::ptr::null_mut()) };
        }

        if status != ERR_SEC_SUCCESS {
            return Err((
                status,
                format!("macOS Keychain SecItemAdd failed (OSStatus {})", status),
            ));
        }
        Ok(())
    }

    pub(super) fn write_generic_password(
        service: &str,
        account: &str,
        secret: &str,
    ) -> Result<(), String> {
        let _lock = USER_INTERACTION_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _no_ui = ScopedNoKeychainUi::enter();

        let service_cf = make_cf_string(service)?;
        let account_cf = make_cf_string(account)?;
        let secret_cf = make_cf_data(secret.as_bytes())?;

        let delete_query = unsafe {
            make_cf_dict(
                &[kSecClass, kSecAttrService, kSecAttrAccount],
                &[
                    kSecClassGenericPassword,
                    service_cf.as_ptr(),
                    account_cf.as_ptr(),
                ],
            )?
        };
        let del_status = unsafe { SecItemDelete(delete_query.as_ptr()) };
        if del_status != ERR_SEC_SUCCESS && del_status != ERR_SEC_ITEM_NOT_FOUND {
            tracing::debug!(
                "SecItemDelete returned OSStatus {} for {}:{}",
                del_status,
                service,
                account
            );
        }

        // 1. Prefer explicit PartitionID ACL for cross-app interop without prompts
        if let Ok(access_cf) = create_unrestricted_access(&service_cf, true) {
            if try_add_generic_password(&service_cf, &account_cf, &secret_cf, Some(&access_cf))
                .is_ok()
            {
                return Ok(());
            }
        }

        // 2. Fall back to unrestricted decrypt ACL without partition
        if let Ok(fallback_access) = create_unrestricted_access(&service_cf, false) {
            if try_add_generic_password(
                &service_cf,
                &account_cf,
                &secret_cf,
                Some(&fallback_access),
            )
            .is_ok()
            {
                return Ok(());
            }
        }

        // 3. Fall back to standard item without custom SecAccessRef (default ACL, completely non-prompting)
        try_add_generic_password(&service_cf, &account_cf, &secret_cf, None).map_err(|(_, msg)| msg)
    }

    pub(super) enum SilentKeychainRead {
        Found(String),
        NotFound,
        NeedsSubprocessFallback,
        Denied(OSStatus),
    }

    pub(super) fn read_generic_password_silent(service: &str, account: &str) -> SilentKeychainRead {
        let Ok(service_cf) = make_cf_string(service) else {
            return SilentKeychainRead::NeedsSubprocessFallback;
        };
        let Ok(account_cf) = make_cf_string(account) else {
            return SilentKeychainRead::NeedsSubprocessFallback;
        };
        let Ok(query) = (unsafe {
            make_cf_dict(
                &[
                    kSecClass,
                    kSecAttrService,
                    kSecAttrAccount,
                    kSecReturnData,
                    kSecUseAuthenticationUI,
                ],
                &[
                    kSecClassGenericPassword,
                    service_cf.as_ptr(),
                    account_cf.as_ptr(),
                    kCFBooleanTrue,
                    kSecUseAuthenticationUIFail,
                ],
            )
        }) else {
            return SilentKeychainRead::NeedsSubprocessFallback;
        };

        // `kSecUseAuthenticationUIFail` only applies to Data Protection keychains on macOS;
        // legacy file-based keychains ignore it and still pop up modal dialogs on partition
        // mismatch. Disable user interaction around `SecItemCopyMatching` so `apple-tool:`
        // items fall back silently to `/usr/bin/security find-generic-password` without prompting.
        let mut result_raw: CFTypeRef = std::ptr::null();
        let status = {
            let _lock = USER_INTERACTION_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let _no_ui = ScopedNoKeychainUi::enter();
            unsafe { SecItemCopyMatching(query.as_ptr(), &mut result_raw) }
        };
        match status {
            ERR_SEC_SUCCESS => {
                let Some(data_guard) = (unsafe { CfGuard::from_create(result_raw) }) else {
                    return SilentKeychainRead::NotFound;
                };
                let len = unsafe { CFDataGetLength(data_guard.as_ptr()) };
                if len <= 0 {
                    return SilentKeychainRead::Found(String::new());
                }
                let ptr = unsafe { CFDataGetBytePtr(data_guard.as_ptr()) };
                if ptr.is_null() {
                    return SilentKeychainRead::Denied(status);
                }
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
                SilentKeychainRead::Found(String::from_utf8_lossy(bytes).trim().to_string())
            }
            ERR_SEC_ITEM_NOT_FOUND => SilentKeychainRead::NotFound,
            ERR_SEC_INTERACTION_NOT_ALLOWED | ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED => {
                SilentKeychainRead::Denied(status)
            }
            _ => SilentKeychainRead::Denied(status),
        }
    }

    #[cfg(test)]
    pub(super) fn delete_generic_password(service: &str, account: &str) {
        let (Ok(service_cf), Ok(account_cf)) = (make_cf_string(service), make_cf_string(account))
        else {
            return;
        };
        if let Ok(query) = unsafe {
            make_cf_dict(
                &[kSecClass, kSecAttrService, kSecAttrAccount],
                &[
                    kSecClassGenericPassword,
                    service_cf.as_ptr(),
                    account_cf.as_ptr(),
                ],
            )
        } {
            let _ = unsafe { SecItemDelete(query.as_ptr()) };
        }
    }
}

#[cfg(target_os = "macos")]
fn write_macos_keychain_generic_password(
    service: &str,
    account: &str,
    secret: &str,
) -> Result<(), String> {
    let res = macos_keychain_ffi::write_generic_password(service, account, secret);
    // Invalidate even if writing fails (e.g. after SecItemDelete) so the coordinator does
    // not serve a stale cached credential.
    invalidate_system_keychain_cache(service, account);
    res
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_generic_password(
    service: &str,
    account: &str,
    bypass_cache: bool,
) -> KeychainLookupOutcome {
    system_keychain_coordinator().read_with(service, account, bypass_cache, || {
        match macos_keychain_ffi::read_generic_password_silent(service, account) {
            macos_keychain_ffi::SilentKeychainRead::Found(secret) => {
                return KeychainLookupOutcome::Found(secret);
            }
            macos_keychain_ffi::SilentKeychainRead::NotFound => {
                return KeychainLookupOutcome::NotFound;
            }
            macos_keychain_ffi::SilentKeychainRead::Denied(status) => {
                let reason = format!(
                    "macOS Keychain SecItemCopyMatching denied (OSStatus {})",
                    status
                );
                crate::modules::logger::log_warn(&format!(
                    "[Keychain] macOS Keychain read denied/canceled for {}:{}; suppressing passive probes until explicit switch/write ({})",
                    service,
                    account,
                    reason
                ));
                return KeychainLookupOutcome::Denied(reason);
            }
            macos_keychain_ffi::SilentKeychainRead::NeedsSubprocessFallback => {}
        }

        match Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", account, "-w"])
            .output()
        {
            Ok(output) if output.status.success() => {
                KeychainLookupOutcome::Found(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let outcome = classify_macos_security_failure(output.status.code(), &stderr);
                if let KeychainLookupOutcome::Denied(ref reason) = outcome {
                    crate::modules::logger::log_warn(&format!(
                        "[Keychain] macOS Keychain read denied/canceled for {}:{}; suppressing passive probes until explicit switch/write ({})",
                        service,
                        account,
                        reason
                    ));
                }
                outcome
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "[Keychain] Failed to invoke /usr/bin/security for {}:{}: {}",
                    service, account, e
                ));
                KeychainLookupOutcome::NotFound
            }
        }
    })
}

#[allow(async_fn_in_trait)]
pub trait SystemIntegration: Send + Sync {
    /// 当切换账号时执行的系统层操作（如杀进程、写入文件、注入数据库）
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String>;

    /// 更新系统托盘（如果适用）
    fn update_tray(&self);

    /// 发送系统通知
    fn show_notification(&self, title: &str, body: &str);

    /// Whether `on_account_switch` already handled WSL credential sync for `target_ide`.
    fn handles_wsl_sync(&self, _target_ide: Option<&str>) -> bool {
        false
    }
}

/// Check if `target_ide` refers to the Antigravity ACP IDE Plugin (`antigravity-acp`).
pub fn is_acp_target(target_ide: Option<&str>) -> bool {
    matches!(target_ide, Some("acp" | "plugin" | "ide-plugin"))
}

fn desktop_handles_wsl_sync(target_ide: Option<&str>) -> bool {
    target_ide != Some("agy") && !is_acp_target(target_ide)
}

/// 根据目标参数、进程运行态及可执行文件存在性决策最终切换环境
pub fn resolve_effective_target(
    target_ide: Option<&str>,
    classic_running: bool,
    ide_running: bool,
    has_classic_exe: bool,
    ide_exe_path: Option<&str>,
) -> (bool, Option<&'static str>) {
    let is_explicit_ide = target_ide == Some("ide");
    let is_explicit_classic = target_ide == Some("classic");

    if is_explicit_ide {
        return (true, Some("ide"));
    }
    if is_explicit_classic {
        return (false, Some("classic"));
    }

    // target_ide 为 None 或未指定时进行智能环境探查（经典版桌面端优先，严禁仅凭静态 IDE 数据库文件劫持经典版目标）
    let mut is_ide = false;
    if classic_running {
        // 原生经典版正在运行，确定目标为经典版
        is_ide = false;
    } else if ide_running {
        // 经典版未运行，但 IDE 正在运行，推导为 IDE
        is_ide = true;
    } else if has_classic_exe {
        // 原生经典版可执行文件存在，优先保持经典版
        is_ide = false;
    } else if let Some(exe_str) = ide_exe_path {
        // 原生经典版不存在，检查是否存在 IDE 可执行文件
        if process::is_antigravity_ide_str(exe_str) {
            is_ide = true;
        }
    }

    let effective = if is_ide {
        Some("ide")
    } else if is_explicit_classic {
        Some("classic")
    } else {
        None
    };

    (is_ide, effective)
}

/// 桌面版实现：包含完整的进程控制 and UI 同步
pub struct DesktopIntegration {
    pub app_handle: tauri::AppHandle,
}

/// 写入账号凭据：>= 2.0.0 的原生应用走系统 Keyring，旧架构与定制 IDE 走 SQLite 注入。
///
/// 抽成独立函数是为了让「热切号」能在终止 language_server 子进程**之前**完成凭据写入
/// （见 `on_account_switch` 的顺序说明），同时让完整重启路径保持原有的「先杀后写」顺序。
fn apply_account_credentials(
    account: &Account,
    effective_target: Option<&str>,
    is_ide: bool,
    active_exe_path: Option<&std::path::Path>,
) -> Result<(), String> {
    let mut use_keyring = false;

    // 硬防护（issue #3762、issue #3770、issue #3778）：
    // 目标一旦确认为 IDE，执行全链路双写与权威对齐：
    // 1. 注入 SQLite state.vscdb（供 VS Code 界面及扩展前端快速恢复会话与显示）
    // 2. 写入系统 Keyring（服务 gemini:antigravity，供 IDE 2.6.0+ 后台核心 agy 语言服务作为首要真理源读取）
    // 3. 同步写入文件级凭据回退 (~/.gemini/jetski-standalone-oauth-token 与 oauth_creds.json)
    let is_target_ide = is_ide
        || effective_target == Some("ide")
        || (effective_target != Some("classic")
            && active_exe_path
                .map(process::is_antigravity_ide_path)
                .unwrap_or(false));

    if is_target_ide {
        crate::modules::logger::log_info(
            "[Desktop] Target is Antigravity IDE (VS Code architecture); performing dual write (SQLite state.vscdb + system Keyring + file fallback).",
        );

        // 1.1 获取存储路径并写入设备 Profile
        if let Ok(storage_path) = device::get_storage_path(effective_target) {
            if let Some(ref profile) = account.device_profile {
                let _ = device::write_profile(&storage_path, profile);
            }
        }

        // 1.2 注入 SQLite 数据库 (state.vscdb)
        let db_path = db::get_db_path(effective_target)?;
        if db_path.exists() {
            let backup_path = db_path.with_extension("vscdb.backup");
            let _ = fs::copy(&db_path, &backup_path);
        }

        db::inject_token(
            &db_path,
            &account.token.access_token,
            &account.token.refresh_token,
            account.token.expiry_timestamp,
            &account.email,
            account.token.is_gcp_tos,
            account.token.project_id.as_deref(),
            account.token.id_token.as_deref(),
            account.token.oauth_client_key.as_deref(),
            effective_target,
        )?;

        if let Some(ref profile) = account.device_profile {
            let _ = db::write_service_machine_id(&db_path, &profile.mac_machine_id);
        }

        // 1.2.1 同步 Electron 经典版/IDE 全局持久化配置 (app_storage.json) 中的条款与协议状态，
        // 彻底消除切号后弹出“One last thing before getting started... / Terms of Service”二次确认卡片 (Fixes #4026)
        device::sync_classic_app_storage_onboarding(
            effective_target,
            &account.email,
            account.token.is_gcp_tos,
        );

        // 1.3 同步写入系统 Keyring (针对 IDE 2.6.0+ agy 后台语言服务鉴权真理源)
        if let Err(keyring_err) = write_to_system_keyring(account) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Warning: Failed to write system Keyring for IDE: {}",
                keyring_err
            ));
        }

        // 1.4 同步写入本地文件凭据回退 (~/.gemini/jetski-standalone-oauth-token 与 oauth_creds.json)
        if let Err(file_err) = write_to_file_credentials(account, false) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Warning: Failed to write file credentials for IDE: {}",
                file_err
            ));
        }

        return Ok(());
    }

    if !is_target_ide {
        // 经典原生版：自动探测版本号（优先使用预快照路径）
        match version::get_antigravity_version_with_path(
            effective_target,
            active_exe_path.as_deref(),
        ) {
            Ok(ver) => {
                // 如果版本号 >= 2.0.0
                if version::compare_version(&ver.short_version, "2.0.0") != std::cmp::Ordering::Less
                {
                    use_keyring = true;
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Detected Antigravity version {} >= 2.0.0, using system Keyring.",
                        ver.short_version
                    ));
                } else {
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Detected Antigravity version {} < 2.0.0, falling back to legacy SQLite injection.",
                        ver.short_version
                    ));
                }
            }
            Err(e) => {
                // 如果探测失败，优先检查本地是否存在可用的 SQLite 数据库 (state.vscdb)
                // 若存在数据库，说明是经典的 VS Code/IDE 架构，优先使用 SQLite 注入，防止无 secret-tool 时报错
                let has_sqlite_db = db::get_db_path(effective_target)
                    .map(|p| p.exists())
                    .unwrap_or(false);

                if has_sqlite_db {
                    use_keyring = false;
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Failed to detect Antigravity version ({}), but detected existing SQLite database. Falling back to SQLite injection.",
                        e
                    ));
                } else {
                    use_keyring = true;
                    crate::modules::logger::log_warn(&format!(
                        "[Desktop] Failed to detect Antigravity version ({}) and no SQLite database found, defaulting to system Keyring.",
                        e
                    ));
                }
            }
        }
    }

    // ================== Antigravity 经典版/原生应用逻辑 ==================
    // 2.1 写入设备 Profile (若 storage_path 可解析)
    if let Ok(storage_path) = device::get_storage_path(effective_target) {
        if let Some(ref profile) = account.device_profile {
            let _ = device::write_profile(&storage_path, profile);
        }
    }

    // 2.1.1 同步 Electron 经典版专属全局持久化配置 (app_storage.json) 中的条款与协议状态，
    // 彻底消除切号后弹出“One last thing before getting started... / Terms of Service”二次确认卡片 (Fixes #4026)
    device::sync_classic_app_storage_onboarding(
        effective_target,
        &account.email,
        account.token.is_gcp_tos,
    );

    // 2.2 双写同步：若经典版存储路径下存在 SQLite state.vscdb，执行数据库 Token 注入
    // 确保原生桌面端与扩展会话状态在所有 Antigravity 变体中保持严格一致 (Fixes #3829)
    let mut sqlite_injected = false;
    let mut sqlite_err_opt = None;
    let db_path_res = db::get_db_path(effective_target);
    match db_path_res {
        Ok(ref db_path) if db_path.exists() => {
            crate::modules::logger::log_info(&format!(
                "[Desktop] Detected existing SQLite DB at {:?} for classic target; performing dual-write.",
                db_path
            ));
            let backup_path = db_path.with_extension("vscdb.backup");
            let _ = fs::copy(db_path, &backup_path);
            match db::inject_token(
                db_path,
                &account.token.access_token,
                &account.token.refresh_token,
                account.token.expiry_timestamp,
                &account.email,
                account.token.is_gcp_tos,
                account.token.project_id.as_deref(),
                account.token.id_token.as_deref(),
                account.token.oauth_client_key.as_deref(),
                effective_target,
            ) {
                Ok(_) => {
                    sqlite_injected = true;
                    if let Some(ref profile) = account.device_profile {
                        let _ = db::write_service_machine_id(db_path, &profile.mac_machine_id);
                    }
                }
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "[Desktop] Failed to inject token into SQLite DB for classic: {}",
                        e
                    ));
                    sqlite_err_opt = Some(e);
                }
            }
        }
        Ok(ref db_path) => {
            if !use_keyring {
                sqlite_err_opt = Some(format!(
                    "SQLite database file not found at {:?} for legacy Antigravity (< 2.0.0)",
                    db_path
                ));
            }
        }
        Err(e) => {
            if !use_keyring {
                sqlite_err_opt = Some(e);
            }
        }
    }

    // 2.3 写入系统 Keychain/Keyring
    let mut keyring_err_opt = None;
    if use_keyring {
        if let Err(keyring_err) = write_to_system_keyring(account) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Keyring write failed ({}) for classic.",
                keyring_err
            ));
            keyring_err_opt = Some(keyring_err);
        }
    }

    // 2.4 同步写入 Standalone / 语言服务原生降级凭据: ~/.gemini/jetski-standalone-oauth-token 与 oauth_creds.json
    // 消除经典款切号后客户端弹出“还需要登录认证”的问题 (Fixes #3829)
    if let Err(file_err) = write_to_file_credentials(account, false) {
        crate::modules::logger::log_warn(&format!(
            "[Desktop] Warning: Failed to write file credentials for classic: {}",
            file_err
        ));
    }

    // 2.5 校验凭据写入状态：
    if use_keyring {
        // 若要求使用 Keyring 且 Keyring 写入失败，但在已成功注入本地 SQLite 数据库时降级允许切号继续；
        // 若两者均未成功则阻断切号流程
        if let Some(keyring_err) = keyring_err_opt {
            if !sqlite_injected {
                return Err(keyring_err);
            }
            crate::modules::logger::log_warn(
                "[Desktop] Keyring write failed, but SQLite DB was synchronized for classic; switch proceeding.",
            );
        }
    } else {
        // 旧版本 (< 2.0.0) 必须成功注入 SQLite
        if !sqlite_injected {
            return Err(sqlite_err_opt.unwrap_or_else(|| {
                "Failed to inject token into SQLite DB for legacy Antigravity (< 2.0.0)".to_string()
            }));
        }
    }

    Ok(())
}

fn execute_desktop_account_switch(
    app_handle: &tauri::AppHandle,
    account: &crate::models::Account,
    target_ide: Option<&str>,
) -> Result<(), String> {
    crate::modules::logger::log_info(&format!(
        "[Desktop] Executing system switch for: {} (target_ide: {:?})",
        account.email, target_ide
    ));

    if target_ide == Some("agy") {
        write_to_system_keyring(account)?;
        let _ = write_to_file_credentials(account, true);
        // A successful write (or a file fallback) is not proof that the CLI
        // will read this account from the system credential store.
        let stored = read_from_system_keyring_only()?;
        verify_agy_credentials(&account.token.refresh_token, &stored.refresh_token)?;

        if let Ok(storage_path) = device::get_storage_path(target_ide) {
            if let Some(ref profile) = account.device_profile {
                let _ = device::write_profile(&storage_path, profile);
            }
        }

        let is_running = process::is_process_running_by_name("agy");
        let msg = if is_running {
            format!(
                "Credentials for {} saved and verified. Running agy sessions may still use and write back their previous credentials.",
                account.email
            )
        } else {
            format!(
                "Credentials for {} saved and verified for the next CLI command.",
                account.email
            )
        };
        crate::modules::logger::log_info(&format!("[Notification] Antigravity CLI: {}", msg));
        let _ = crate::modules::tray::update_tray_menus(app_handle);

        return Ok(());
    }

    if is_acp_target(target_ide) {
        let killed = switch_acp_account(account)?;
        let msg = if killed > 0 {
            format!(
                "Credentials for {} saved and verified. Restarted {} running agy_acp_server process(es).",
                account.email, killed
            )
        } else {
            format!(
                "Credentials for {} saved and verified for the Antigravity IDE Plugin (ACP).",
                account.email
            )
        };
        crate::modules::logger::log_info(&format!(
            "[Notification] Antigravity IDE Plugin: {}",
            msg
        ));
        let _ = crate::modules::tray::update_tray_menus(app_handle);

        return Ok(());
    }

    // 1. 智能决策：判断目标是 Antigravity IDE (VS Code 定制版) 还是 Antigravity 经典版 (原生桌面端)
    let classic_running = process::is_antigravity_running(None);
    let ide_running = process::is_antigravity_running(Some("ide"));
    let classic_exe = process::get_antigravity_executable_path(Some("classic"));
    let ide_exe = process::get_antigravity_executable_path(Some("ide"));
    let ide_exe_str = ide_exe.as_ref().map(|p| p.to_string_lossy().to_string());

    let (is_ide, effective_target) = resolve_effective_target(
        target_ide,
        classic_running,
        ide_running,
        classic_exe.is_some(),
        ide_exe_str.as_deref(),
    );

    if is_ide {
        crate::modules::logger::log_info(
            "[Desktop] Determined target environment is Antigravity IDE, using IDE account switch logic.",
        );
    } else {
        crate::modules::logger::log_info(
            "[Desktop] Determined target environment is Antigravity classic, using classic account switch logic.",
        );
    }

    // 0. 在关闭外部进程前，预先快照捕获正在运行的客户端可执行文件路径与启动参数
    // 彻底防止杀死进程后由于安装在非标准路径而丢失路径导致启动失败 (Unable to start)
    let active_exe_path = process::get_antigravity_executable_path(effective_target);
    let active_args = process::get_args_from_running_process(effective_target);

    // 2. 决定切换模式（热切号 / 完整重启）并处理进程与凭据
    //
    //    · 热切号（原生经典版与 Antigravity IDE 统一支持，issue #3503 / issue #3637）：
    //      主窗口进程（无论是 Standalone 还是 VS Code 架构的 IDE）与后台 language_server/agy 解耦，
    //      只要客户端运行中且能定位到语言服务子进程，先将凭据双写至 SQLite/Keyring/本地文件，
    //      然后仅终止后台 language_server/agy 子进程。宿主内置的 supervisor 会在约 1.5~2 秒内原地
    //      重建子进程并重新挂载新凭据，完美保留主窗口和未保存内容，无需重启 IDE。
    //      顺序是**先写凭据 → 再杀子进程 →（best-effort）再补写一次**。
    //    · 完整重启 —— 客户端未运行、未定位到语言服务子进程或杀死/拉起失败：
    //      兜底保持「终止主进程 → 干净写入凭据 → 重新启动客户端」的稳健流程。
    let running = process::is_antigravity_running(effective_target);
    let mut hot_switch = false;
    let mut credentials_applied = false;
    let mut killed_subprocesses = Vec::new();

    // 只要目标客户端正在运行，统一优先尝试无感热切号
    if running {
        apply_account_credentials(
            account,
            effective_target,
            is_ide,
            active_exe_path.as_deref(),
        )?;
        credentials_applied = true;

        // 热切号保障（Fixes #4026）：在终止 language_server 子进程前，
        // 验证文件级凭据回退（~/.gemini/jetski-standalone-oauth-token）已持久化到磁盘。
        // 背景：Linux D-Bus/Keyring 存在异步延迟（尤其是 Wayland/AppImage 沙箱），
        // 当 Keyring 在 10s 窗口内返回 has_token=false 时，前端 authActor 会切换到
        // signedOut 状态并将 URL 污染为 /onboarding?login=true，而 Electron 的
        // buildLsRestartWindowUrl 会将此污染 URL 传播给新启动的 language_server。
        // 通过确保文件凭据在杀死子进程前真实可读，supervisor 重建的 language_server
        // 始终能从文件回退路径获取 has_token=true，彻底阻断 URL 污染链。
        // 此验证不阻塞 macOS/Windows（它们的凭据路径始终同步可用）。
        ensure_file_credentials_flushed(account);

        match process::kill_language_server_subprocesses_and_get_pids(effective_target) {
            Ok(pids) if !pids.is_empty() => {
                hot_switch = true;
                let n = pids.len();
                killed_subprocesses = pids;
                crate::modules::logger::log_info(&format!(
                    "[Desktop] Hot switch: terminated {} language_server/agy subprocess(es) for {:?}; \
                     main window kept alive, supervisor will respawn the child with the new credentials.",
                    n, effective_target
                ));

                // 补写一次凭据（best-effort）：子进程已被终止，此时数据库不存在竞争写者，
                // 这次写入即为权威值 —— 用于覆盖"写凭据 → 杀子进程"这几十毫秒窗口内
                // 旧语言服务可能把旧 token 回写进 Keyring 的情况，
                // 确保 supervisor 约 2s 后重新拉起的子进程读到的一定是新凭据。
                // 失败只告警，不使整个切号失败：第一次写入已经就位。
                if let Err(e) = apply_account_credentials(
                    account,
                    effective_target,
                    is_ide,
                    active_exe_path.as_deref(),
                ) {
                    crate::modules::logger::log_warn(&format!(
                        "[Desktop] Hot switch: post-kill credential re-assert failed ({}); \
                         the pre-kill write is already in place, continuing.",
                        e
                    ));
                }
            }
            Ok(_) => crate::modules::logger::log_info(
                "[Desktop] Hot switch unavailable: no language_server subprocess found; falling back to full restart.",
            ),
            Err(e) => crate::modules::logger::log_warn(&format!(
                "[Desktop] Hot switch failed ({}); falling back to full restart.",
                e
            )),
        }
    }

    if !hot_switch && running {
        process::close_antigravity(20, effective_target)?;
        // 完整重启路径必须在彻底终止主进程之后重新断言写入凭据，
        // 消除外部客户端退出钩子在关机瞬间回写陈旧 Session Token 的竞态
        apply_account_credentials(
            account,
            effective_target,
            is_ide,
            active_exe_path.as_deref(),
        )?;
        credentials_applied = true;
    }
    if effective_target != target_ide
        && target_ide.is_some()
        && process::is_antigravity_running(target_ide)
    {
        process::close_antigravity(20, target_ide)?;
    }

    // 若此前从未写入凭据（例如客户端未处于运行状态）：此时写入凭据
    if !credentials_applied {
        apply_account_credentials(
            account,
            effective_target,
            is_ide,
            active_exe_path.as_deref(),
        )?;
    }

    // 3. 重启外部进程（优先使用预快照路径与启动参数）
    //    热切号路径下主窗口仍然存活：只需等 supervisor 把 language_server 拉起即可，
    //    绝不能再去启动一次主窗口（否则会变成双实例）。
    if hot_switch {
        if process::wait_for_language_server_respawn_excluding(
            effective_target,
            15,
            &killed_subprocesses,
        ) {
            #[cfg(target_os = "windows")]
            crate::modules::wsl_sync::sync_account_to_wsl_blocking(
                account,
                std::time::Duration::from_secs(5),
            );
            crate::modules::logger::log_info(&format!(
                "[Desktop] Hot switch completed for {}: language_server respawned with the new credentials.",
                account.email
            ));
            let _ = crate::modules::tray::update_tray_menus(app_handle);
            return Ok(());
        }

        // 子进程迟迟未恢复 → 降级为完整重启，避免留下"窗口活着但 AI 引擎已死"的残状态
        crate::modules::logger::log_warn(
            "[Desktop] Hot switch degraded: language_server did not respawn within timeout; performing a full restart.",
        );
        process::close_antigravity(20, effective_target)?;
        // 彻底杀死主窗口后重新断言写入凭据，消除退出钩子回写旧 token
        let _ = apply_account_credentials(
            account,
            effective_target,
            is_ide,
            active_exe_path.as_deref(),
        );
    }

    // 在 Windows 宿主机重新拉起 IDE 之前先有界同步 WSL 凭据（Ref #4032），
    // 避免 Remote-WSL 窗口启动瞬间抢先连接并使用旧凭据。
    #[cfg(target_os = "windows")]
    crate::modules::wsl_sync::sync_account_to_wsl_blocking(
        account,
        std::time::Duration::from_secs(5),
    );

    if let Err(e) = process::start_antigravity_with_fallback_path(
        effective_target,
        active_exe_path.as_deref(),
        active_args.as_deref(),
    ) {
        // 若切号前外部客户端原本就没有处于运行状态，且启动失败原因是找不到客户端可执行文件
        // （例如纯反代服务模式、未安装 GUI 客户端或无头环境）：
        // 此时凭据和配置已经写入成功，降级处理并记录信息，避免让整个切号操作报错中断。
        if !running && process::is_client_executable_missing(&e) {
            crate::modules::logger::log_info(
                "[Desktop] Client executable not found and was not running before switch; credentials applied successfully.",
            );
        } else {
            return Err(e);
        }
    }

    // 4. 更新托盘
    let _ = crate::modules::tray::update_tray_menus(app_handle);

    Ok(())
}

impl SystemIntegration for DesktopIntegration {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        let app_handle = self.app_handle.clone();
        let account_owned = account.clone();
        let target_owned = target_ide.map(str::to_string);

        tokio::task::spawn_blocking(move || {
            execute_desktop_account_switch(&app_handle, &account_owned, target_owned.as_deref())
        })
        .await
        .map_err(|e| format!("Account switch blocking task failed: {}", e))?
    }

    fn update_tray(&self) {
        let _ = crate::modules::tray::update_tray_menus(&self.app_handle);
    }

    fn show_notification(&self, title: &str, body: &str) {
        // 使用 tauri-plugin-dialog 或原生通知（此处简化）
        crate::modules::logger::log_info(&format!("[Notification] {}: {}", title, body));
    }

    fn handles_wsl_sync(&self, target_ide: Option<&str>) -> bool {
        desktop_handles_wsl_sync(target_ide)
    }
}

/// 辅助方法：向宿主操作系统的 Keychain/Credentials Manager 写入 Token
pub(crate) fn write_to_system_keyring(account: &crate::models::Account) -> Result<(), String> {
    // 1. 构建 Token 的 JSON Payload，并将过期时间戳格式化为符合 RFC3339 的带微秒格式
    let expiry_secs = if account.token.expiry_timestamp > 10_000_000_000 {
        account.token.expiry_timestamp / 1000
    } else {
        account.token.expiry_timestamp
    };
    let expiry_datetime =
        chrono::DateTime::from_timestamp(expiry_secs, 0).unwrap_or_else(|| chrono::Utc::now());
    let expiry_str = expiry_datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    #[derive(serde::Serialize)]
    struct KeyringTokenDetails {
        access_token: String,
        token_type: String,
        refresh_token: String,
        expiry: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_in: Option<i64>,
    }

    #[derive(serde::Serialize)]
    struct KeyringPayload {
        token: KeyringTokenDetails,
        auth_method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token_email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token: Option<String>,
    }

    let payload_json = serde_json::to_string(&KeyringPayload {
        token: KeyringTokenDetails {
            access_token: account.token.access_token.clone(),
            token_type: "Bearer".to_string(),
            refresh_token: account.token.refresh_token.clone(),
            expiry: expiry_str,
            expires_in: Some(3599),
        },
        auth_method: "consumer".to_string(),
        id_token_email: Some(account.email.clone()),
        id_token: account.token.id_token.clone(),
    })
    .map_err(|e| format!("Failed to serialize keyring JSON: {}", e))?;

    crate::modules::logger::log_info(&format!(
        "[Desktop] Writing token to system credential store for: {}",
        account.email
    ));

    // 2. 跨平台凭据注入
    #[cfg(target_os = "macos")]
    {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let encoded_payload = STANDARD.encode(&payload_json);
        let full_keyring_value = format!("go-keyring-base64:{}", encoded_payload);

        // 2.1 macOS Keychain Access (via native Security.framework FFI so tokens never hit argv or 4KB stdin limits)
        write_macos_keychain_generic_password("gemini", "antigravity", &full_keyring_value)?;
    }

    #[cfg(target_os = "windows")]
    {
        // 2.2 Windows Credential Manager direct Win32 API calls to write raw UTF-8 bytes
        use std::os::windows::ffi::OsStrExt;
        use std::ptr;

        #[repr(C)]
        struct FILETIME {
            dw_low_date_time: u32,
            dw_high_date_time: u32,
        }

        #[repr(C)]
        struct CREDENTIALW {
            flags: u32,
            cred_type: u32,
            target_name: *const u16,
            comment: *const u16,
            last_written: FILETIME,
            credential_blob_size: u32,
            credential_blob: *const u8,
            persist: u32,
            attribute_count: u32,
            attributes: *const std::ffi::c_void,
            target_alias: *const u16,
            user_name: *const u16,
        }

        #[link(name = "advapi32")]
        extern "system" {
            fn CredWriteW(credential: *const CREDENTIALW, flags: u32) -> i32;
            fn CredDeleteW(target_name: *const u16, type_: u32, flags: u32) -> i32;
        }

        let target = "gemini:antigravity";
        let user = "antigravity";
        let secret = payload_json.as_bytes();

        let target_wide: Vec<u16> = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let user_wide: Vec<u16> = std::ffi::OsStr::new(user)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let cred = CREDENTIALW {
            flags: 0,
            cred_type: 1, // CRED_TYPE_GENERIC
            target_name: target_wide.as_ptr(),
            comment: ptr::null(),
            last_written: FILETIME {
                dw_low_date_time: 0,
                dw_high_date_time: 0,
            },
            credential_blob_size: secret.len() as u32,
            credential_blob: secret.as_ptr(),
            persist: 2, // CRED_PERSIST_LOCAL_MACHINE
            attribute_count: 0,
            attributes: ptr::null(),
            target_alias: ptr::null(),
            user_name: user_wide.as_ptr(),
        };

        unsafe {
            // Delete first to ensure we write clean
            let _ = CredDeleteW(target_wide.as_ptr(), 1, 0);

            let res = CredWriteW(&cred, 0);
            if res == 0 {
                let err = std::io::Error::last_os_error();
                return Err(format!("Windows CredWriteW failed: {}", err));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 2.3 Linux Secret Service API
        // [FIX #3418] 在 Linux GNOME 环境下，Secret Service 往往同时存在 'login' 集合与 'default' 集合。
        // agy CLI 读取凭据时严格从 'login' 集合检索。若未指定 --collection，secret-tool 会写入 default 集合，
        // 导致两个集合内容分叉，agy 持续读取到 login 集合中的旧账号。
        // 此处封装辅助函数：优先写入 login 集合，同时确保与 default 集合同步。
        use std::io::Write;
        use std::sync::mpsc;

        let store_to_collection = |collection_opt: Option<&str>,
                                   payload: &[u8]|
         -> Result<(), String> {
            let mut cmd = Command::new("secret-tool");
            cmd.arg("store");
            if let Some(col) = collection_opt {
                cmd.arg(format!("--collection={}", col));
                cmd.arg("--label=Password for 'antigravity' on 'gemini'");
            } else {
                cmd.arg("--label=gemini");
            }
            cmd.args(["service", "gemini", "username", "antigravity"]);
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        return Err(
                            "Linux Secret Service utility 'secret-tool' not found (未检测到 secret-tool 工具)。\n\
                             Please install libsecret-tools to enable Keyring credential storage:\n\
                             • Ubuntu / Debian: sudo apt install -y libsecret-tools\n\
                             • Fedora / RHEL: sudo dnf install -y libsecret\n\
                             • Arch Linux: sudo pacman -S libsecret"
                                .to_string(),
                        );
                    }
                    return Err(format!("Failed to spawn secret-tool: {}", e));
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(payload)
                    .map_err(|e| format!("Failed to write to secret-tool stdin: {}", e))?;
            }

            let child_pid = child.id();
            let (tx, rx) = mpsc::channel::<Result<std::process::Output, std::io::Error>>();
            std::thread::spawn(move || {
                let _ = tx.send(child.wait_with_output());
            });

            let output = match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(result) => {
                    result.map_err(|e| format!("Failed to wait for secret-tool: {}", e))?
                }
                Err(_) => {
                    let _ = Command::new("kill")
                        .args(["-9", &child_pid.to_string()])
                        .output();
                    crate::modules::logger::log_error(
                        "[Desktop] secret-tool store blocked for >10s — D-Bus session bus unreachable.",
                    );
                    return Err(
                        "Keyring write timed out (10s). The D-Bus session bus is not reachable from this process."
                            .to_string(),
                    );
                }
            };

            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Linux secret-tool failed: {}", err_msg.trim()));
            }

            Ok(())
        };

        // 1. 优先尝试写入 'login' 集合（agy CLI 所需）
        let login_res = store_to_collection(
            Some("/org/freedesktop/secrets/collection/login"),
            payload_json.as_bytes(),
        );

        // 2. 同时写入默认集合（保证其他依赖 default collection 的系统工具也能读取）
        let default_res = store_to_collection(None, payload_json.as_bytes());

        // 尝试优先同步写入本地文件凭据 (~/.gemini/oauth_creds.json)
        let _ = write_to_file_credentials(account, false);

        // Regardless of write outcome, clear any cached lookup so subsequent reads re-probe Secret Service.
        invalidate_system_keychain_cache("gemini", "antigravity");

        // 若两者均失败，则返回错误；若至少一个成功，则记录并继续
        if login_res.is_err() && default_res.is_err() {
            return Err(login_res.unwrap_err());
        } else if let Err(e) = login_res {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Failed to write token to 'login' collection, falling back to default collection: {}",
                e
            ));
        } else {
            crate::modules::logger::log_info(
                "[Desktop] Successfully synced credential to Secret Service 'login' collection.",
            );
        }
    }

    crate::modules::logger::log_info(
        "[Desktop] Successfully wrote token to system credential store.",
    );

    // 同步写入 ~/.gemini/ 目录下的文件凭据，兼容 SSH 会话、容器环境和无 Keyring/D-Bus 场景
    if let Err(e) = write_to_file_credentials(account, false) {
        crate::modules::logger::log_warn(&format!("[Desktop] File credential sync warning: {}", e));
    }

    Ok(())
}

/// 辅助方法：向指定目录同步写入本地文件凭据
pub(crate) fn write_to_file_credentials_to_dir(
    gemini_dir: &std::path::Path,
    account: &crate::models::Account,
    sync_agy_cli: bool,
) -> Result<(), String> {
    if !gemini_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(gemini_dir) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Failed to create .gemini directory: {}",
                e
            ));
            return Err(format!("Failed to create .gemini directory: {}", e));
        }
    }

    let expiry_secs = if account.token.expiry_timestamp > 10_000_000_000 {
        account.token.expiry_timestamp / 1000
    } else {
        account.token.expiry_timestamp
    };
    let expiry_datetime =
        chrono::DateTime::from_timestamp(expiry_secs, 0).unwrap_or_else(|| chrono::Utc::now());
    let expiry_str = expiry_datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    #[derive(serde::Serialize)]
    struct AgyTokenDetails {
        access_token: String,
        token_type: String,
        refresh_token: String,
        expiry: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_in: Option<i64>,
    }

    #[derive(serde::Serialize)]
    struct AgyOAuthTokenFile {
        token: AgyTokenDetails,
        auth_method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token_email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token: Option<String>,
    }

    let agy_token = AgyOAuthTokenFile {
        token: AgyTokenDetails {
            access_token: account.token.access_token.clone(),
            token_type: "Bearer".to_string(),
            refresh_token: account.token.refresh_token.clone(),
            expiry: expiry_str,
            expires_in: Some(3599),
        },
        auth_method: "consumer".to_string(),
        id_token_email: Some(account.email.clone()),
        id_token: account.token.id_token.clone(),
    };

    // 1. 同步写入 Standalone / IDE 语言服务原生降级凭据: ~/.gemini/jetski-standalone-oauth-token
    // 当系统 Keyring 不可用或超时时，agy 语言服务以此文件作为首要回退
    let standalone_token_path = gemini_dir.join("jetski-standalone-oauth-token");
    if let Ok(standalone_json_str) = serde_json::to_string_pretty(&agy_token) {
        if let Err(e) = std::fs::write(&standalone_token_path, &standalone_json_str) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Failed to write jetski-standalone-oauth-token: {}",
                e
            ));
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &standalone_token_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
            crate::modules::logger::log_info(&format!(
                "[Desktop] Successfully synced fallback credentials to ~/.gemini/jetski-standalone-oauth-token for: {}",
                account.email
            ));
        }
    }

    // 2. 同步写入 Antigravity CLI (agy) 原生文件凭据: ~/.gemini/antigravity-cli/antigravity-oauth-token
    // 兼容 SSH 会话、tmux、Docker 容器以及无 D-Bus 桌面环境
    // [FIX #3728] 仅在切换 agy CLI 时同步写入该专属凭证，严禁经典款切号盲目覆盖破坏多端账号隔离
    if sync_agy_cli {
        let agy_cli_dir = gemini_dir.join("antigravity-cli");
        if !agy_cli_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&agy_cli_dir) {
                crate::modules::logger::log_warn(&format!(
                    "[Desktop] Failed to create antigravity-cli directory: {}",
                    e
                ));
            }
        }

        let agy_token_path = agy_cli_dir.join("antigravity-oauth-token");
        if let Ok(agy_json_str) = serde_json::to_string_pretty(&agy_token) {
            if let Err(e) = std::fs::write(&agy_token_path, agy_json_str) {
                crate::modules::logger::log_warn(&format!(
                    "[Desktop] Failed to write antigravity-oauth-token: {}",
                    e
                ));
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &agy_token_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                crate::modules::logger::log_info(&format!(
                    "[Desktop] Successfully synced file-based credentials to ~/.gemini/antigravity-cli/antigravity-oauth-token for: {}",
                    account.email
                ));
            }
        }
    }

    // 2. 同步写入 Gemini CLI 凭据 (~/.gemini/oauth_creds.json 以及 ~/.gemini/google_accounts.json)
    let expiry_ms = if account.token.expiry_timestamp > 10_000_000_000 {
        account.token.expiry_timestamp
    } else {
        account.token.expiry_timestamp * 1000
    };

    #[derive(serde::Serialize)]
    struct OAuthCredsFile {
        access_token: String,
        refresh_token: String,
        token_type: String,
        expiry_date: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token: Option<String>,
        scope: String,
    }

    let creds = OAuthCredsFile {
        access_token: account.token.access_token.clone(),
        refresh_token: account.token.refresh_token.clone(),
        token_type: "Bearer".to_string(),
        expiry_date: expiry_ms,
        id_token: account.token.id_token.clone(),
        scope: "https://www.googleapis.com/auth/userinfo.email openid https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.profile".to_string(),
    };

    let creds_path = gemini_dir.join("oauth_creds.json");
    let json_str = serde_json::to_string_pretty(&creds)
        .map_err(|e| format!("Failed to serialize oauth_creds JSON: {}", e))?;

    if let Err(e) = std::fs::write(&creds_path, json_str) {
        crate::modules::logger::log_warn(&format!(
            "[Desktop] Failed to write oauth_creds.json: {}",
            e
        ));
        return Err(format!("Failed to write oauth_creds.json: {}", e));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600));
    }

    #[derive(serde::Serialize)]
    struct GoogleAccountsFile {
        active: String,
        old: Vec<String>,
    }

    let accounts_info = GoogleAccountsFile {
        active: account.email.clone(),
        old: vec![],
    };

    let accounts_path = gemini_dir.join("google_accounts.json");
    if let Ok(accounts_json_str) = serde_json::to_string_pretty(&accounts_info) {
        let _ = std::fs::write(&accounts_path, accounts_json_str);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&accounts_path, std::fs::Permissions::from_mode(0o600));
        }
    }

    crate::modules::logger::log_info(&format!(
        "[Desktop] Successfully synced file-based credentials to ~/.gemini/oauth_creds.json for: {}",
        account.email
    ));

    Ok(())
}

/// 热切号文件凭据落盘保障（Fixes #4026）
///
/// 在热切号路径终止 language_server 子进程之前调用，确认
/// `~/.gemini/jetski-standalone-oauth-token` 已被操作系统真实持久化并可读。
///
/// 策略：
///   1. 读取文件并验证其非空、包含新账号 email，确认写入已生效。
///   2. 若首次读取不符（极端情况：内核缓冲区未提交），重试一次写入，
///      最多等待 100ms，然后继续（文件写入失败不应阻断整个热切号流程，
///      Keyring 或 SQLite 仍可能提供有效凭据）。
///   3. 此函数对 macOS / Windows 无额外开销：写入总是同步完成，
///      读取验证在微秒级返回。
fn ensure_file_credentials_flushed(account: &crate::models::Account) {
    let gemini_dir = match dirs::home_dir() {
        Some(h) => h.join(".gemini"),
        None => return,
    };
    let token_path = gemini_dir.join("jetski-standalone-oauth-token");

    // 读取并验证文件是否包含新账号凭据
    let verified = std::fs::read_to_string(&token_path)
        .ok()
        .filter(|s| !s.is_empty() && s.contains(&account.email))
        .is_some();

    if !verified {
        // 文件尚不可见或内容仍为旧账号数据：重试写入一次
        crate::modules::logger::log_warn(
            "[Desktop] Hot switch: jetski-standalone-oauth-token not yet flushed; retrying write before kill.",
        );
        if let Err(e) = write_to_file_credentials(account, false) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Hot switch: file credential retry write failed ({}); \
                 Keyring/SQLite paths will serve as fallback.",
                e
            ));
            return;
        }

        // 短暂等待让内核提交写入（最多 100ms），然后做最终验证
        std::thread::sleep(std::time::Duration::from_millis(100));
        let final_ok = std::fs::read_to_string(&token_path)
            .ok()
            .filter(|s| !s.is_empty() && s.contains(&account.email))
            .is_some();
        if final_ok {
            crate::modules::logger::log_info(
                "[Desktop] Hot switch: jetski-standalone-oauth-token confirmed flushed after retry.",
            );
        } else {
            crate::modules::logger::log_warn(
                "[Desktop] Hot switch: jetski-standalone-oauth-token still not confirmed after retry; \
                 proceeding — Keyring/SQLite paths remain active.",
            );
        }
    } else {
        crate::modules::logger::log_info(&format!(
            "[Desktop] Hot switch: jetski-standalone-oauth-token confirmed flushed for {}; \
             safe to terminate language_server.",
            account.email
        ));
    }
}

/// 辅助方法：同步写入本地文件凭据 (~/.gemini/antigravity-cli/antigravity-oauth-token 以及 ~/.gemini/oauth_creds.json)
/// 用于在 SSH 会话、容器环境或无系统 Keyring / D-Bus 的场景下保障 CLI/工具的凭据兼容性
fn write_to_file_credentials(
    account: &crate::models::Account,
    sync_agy_cli: bool,
) -> Result<(), String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Err("Failed to resolve user home directory".to_string()),
    };
    write_to_file_credentials_to_dir(&home.join(".gemini"), account, sync_agy_cli)
}

/// 辅助方法：从指定目录的本地文件凭据读取 Token 作为跨平台回退
fn read_from_file_credentials_from_dir(
    gemini_dir: &std::path::Path,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    // 1. 优先尝试从 Antigravity CLI (agy) 原生文件凭据或 Standalone 回退凭据读取
    let candidate_paths = [
        gemini_dir
            .join("antigravity-cli")
            .join("antigravity-oauth-token"),
        gemini_dir.join("jetski-standalone-oauth-token"),
    ];
    for path in &candidate_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(state) = parse_keyring_payload(&content) {
                    return Ok(state);
                }
            }
        }
    }

    // 2. 回退到 oauth_creds.json
    let creds_path = gemini_dir.join("oauth_creds.json");
    if !creds_path.exists() {
        return Err(
            "No file-based credentials found (~/.gemini/antigravity-cli/antigravity-oauth-token or ~/.gemini/oauth_creds.json)"
                .to_string(),
        );
    }
    let content = fs::read_to_string(&creds_path)
        .map_err(|e| format!("Failed to read oauth_creds.json: {}", e))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse oauth_creds.json: {}", e))?;
    let refresh_token = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Refresh token not found in oauth_creds.json".to_string())?
        .to_string();
    Ok(crate::modules::migration::ImportedOAuthState {
        refresh_token,
        is_gcp_tos: true,
        project_id: None,
    })
}

/// 辅助方法：从本地文件凭据读取 Token 作为跨平台回退
fn read_from_file_credentials() -> Result<crate::modules::migration::ImportedOAuthState, String> {
    let home =
        dirs::home_dir().ok_or_else(|| "Failed to resolve user home directory".to_string())?;
    read_from_file_credentials_from_dir(&home.join(".gemini"))
}

/// 读取 Antigravity CLI (agy) 原生与备份文件凭据，严格限制仅扫描 Antigravity
pub fn read_antigravity_cli_credentials() -> Vec<crate::modules::migration::ImportedOAuthState> {
    let mut states = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return states;
    };

    let cli_dir = home.join(".gemini").join("antigravity-cli");
    let candidate_files = [
        cli_dir.join("antigravity-oauth-token"),
        cli_dir.join("antigravity-oauth-token.antigravity.bak"),
        cli_dir.join("antigravity-oauth-token.bak"),
        home.join(".gemini").join("jetski-standalone-oauth-token"),
    ];

    let mut seen_tokens = std::collections::HashSet::new();
    for path in &candidate_files {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(state) = parse_keyring_payload(&content) {
                    if !state.refresh_token.is_empty()
                        && seen_tokens.insert(state.refresh_token.clone())
                    {
                        tracing::debug!(
                            "[CLI-Scan] Discovered Antigravity CLI OAuth state in {:?}",
                            path
                        );
                        states.push(state);
                    }
                }
            }
        }
    }

    states
}

/// 辅助方法：从宿主操作系统的 Keychain/Credentials Manager 读取 Token
pub fn read_from_system_keyring() -> Result<crate::modules::migration::ImportedOAuthState, String> {
    read_system_credentials(true, false)
}

/// Read only the system store: stale fallback files must not confirm an agy switch.
/// Bypasses the passive read cache so post-switch verification and explicit imports are fresh.
pub(crate) fn read_from_system_keyring_only(
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    read_system_credentials(false, true)
}

/// Passive status detection reader for system credentials:
/// uses single-flight coordination, short TTL cache, and denial cooldown on macOS,
/// falling back cleanly to passwordless local credential files when Keychain is denied or absent.
pub(crate) fn read_from_system_keyring_passive(
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    read_system_credentials(true, false)
}

fn verify_agy_credentials(expected: &str, stored: &str) -> Result<(), String> {
    if expected.is_empty() || expected != stored {
        return Err(
            "Stored agy credentials do not match the selected account; the switch was not confirmed."
                .to_string(),
        );
    }
    Ok(())
}

fn read_system_credentials(
    allow_file_fallback: bool,
    bypass_cache: bool,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    #[cfg(target_os = "windows")]
    let _ = bypass_cache;

    #[cfg(target_os = "macos")]
    {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let secret_str =
            match read_macos_keychain_generic_password("gemini", "antigravity", bypass_cache) {
                KeychainLookupOutcome::Found(secret) => secret,
                KeychainLookupOutcome::NotFound | KeychainLookupOutcome::Denied(_) => {
                    if allow_file_fallback {
                        if let Ok(file_state) = read_from_file_credentials() {
                            return Ok(file_state);
                        }
                    }
                    return Err("No credential found in macOS Keychain".to_string());
                }
            };

        let payload_str = if secret_str.starts_with("go-keyring-base64:") {
            let b64_part = &secret_str["go-keyring-base64:".len()..];
            let decoded = STANDARD
                .decode(b64_part)
                .map_err(|e| format!("Base64 decode failed: {}", e))?;
            String::from_utf8(decoded).map_err(|e| format!("UTF-8 decode failed: {}", e))?
        } else {
            secret_str
        };

        return parse_keyring_payload(&payload_str);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr;

        #[repr(C)]
        struct FILETIME {
            dw_low_date_time: u32,
            dw_high_date_time: u32,
        }

        #[repr(C)]
        struct CREDENTIALW {
            flags: u32,
            cred_type: u32,
            target_name: *const u16,
            comment: *const u16,
            last_written: FILETIME,
            credential_blob_size: u32,
            credential_blob: *mut u8,
            persist: u32,
            attribute_count: u32,
            attributes: *const std::ffi::c_void,
            target_alias: *const u16,
            user_name: *const u16,
        }

        #[link(name = "advapi32")]
        extern "system" {
            fn CredReadW(
                target_name: *const u16,
                type_: u32,
                flags: u32,
                credential: *mut *mut CREDENTIALW,
            ) -> i32;
            fn CredFree(buffer: *mut std::ffi::c_void);
        }

        let target = "gemini:antigravity";
        let target_wide: Vec<u16> = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut cred_ptr: *mut CREDENTIALW = ptr::null_mut();
        unsafe {
            let res = CredReadW(target_wide.as_ptr(), 1, 0, &mut cred_ptr);
            if res == 0 || cred_ptr.is_null() {
                if allow_file_fallback {
                    if let Ok(file_state) = read_from_file_credentials() {
                        return Ok(file_state);
                    }
                }
                return Err("No credential found in Windows Credential Manager".to_string());
            }

            let cred = &*cred_ptr;
            let blob = std::slice::from_raw_parts(
                cred.credential_blob,
                cred.credential_blob_size as usize,
            );
            let payload_str = String::from_utf8_lossy(blob).to_string();
            CredFree(cred_ptr as *mut std::ffi::c_void);

            return parse_keyring_payload(&payload_str);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let outcome = system_keychain_coordinator().read_with(
            "gemini",
            "antigravity",
            bypass_cache,
            || match Command::new("secret-tool")
                .args(["lookup", "service", "gemini", "username", "antigravity"])
                .output()
            {
                Ok(out) if out.status.success() => {
                    KeychainLookupOutcome::Found(String::from_utf8_lossy(&out.stdout).trim().to_string())
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    classify_linux_secret_tool_failure(out.status.code(), &stderr)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    KeychainLookupOutcome::Denied(
                        "Linux Secret Service utility 'secret-tool' not found (未检测到 secret-tool 工具).\n\
                         Please install libsecret-tools to enable Keyring storage:\n\
                         • Ubuntu / Debian: sudo apt install -y libsecret-tools\n\
                         • Fedora / RHEL: sudo dnf install -y libsecret\n\
                         • Arch Linux: sudo pacman -S libsecret"
                            .to_string(),
                    )
                }
                Err(e) => KeychainLookupOutcome::Denied(format!(
                    "Failed to execute secret-tool: {}",
                    e
                )),
            },
        );

        match outcome {
            KeychainLookupOutcome::Found(payload_str) => {
                return parse_keyring_payload(&payload_str)
            }
            KeychainLookupOutcome::NotFound => {
                if allow_file_fallback {
                    if let Ok(file_state) = read_from_file_credentials() {
                        return Ok(file_state);
                    }
                }
                return Err("No credential found in Linux secret-tool".to_string());
            }
            KeychainLookupOutcome::Denied(reason) => {
                if allow_file_fallback {
                    if let Ok(file_state) = read_from_file_credentials() {
                        return Ok(file_state);
                    }
                }
                return Err(reason);
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("Keyring not supported on this operating system".to_string())
    }
}

fn parse_keyring_payload(
    payload_str: &str,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    let json: serde_json::Value = serde_json::from_str(payload_str)
        .map_err(|e| format!("Failed to parse keyring payload JSON: {}", e))?;

    let refresh_token = json
        .get("token")
        .and_then(|t| t.get("refresh_token"))
        .and_then(|v| v.as_str())
        .or_else(|| json.get("refresh_token").and_then(|v| v.as_str()))
        .ok_or_else(|| "Refresh Token not found in keyring payload".to_string())?
        .to_string();

    let project_id = json
        .get("project_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(crate::modules::migration::ImportedOAuthState {
        refresh_token,
        is_gcp_tos: true,
        project_id,
    })
}

/// Resolve the Antigravity ACP IDE Plugin directory (`$GEMINI_HOME/antigravity-acp` or `~/.gemini/antigravity-acp`).
fn resolve_acp_dir() -> Result<std::path::PathBuf, String> {
    if let Ok(gemini_home) = std::env::var("GEMINI_HOME") {
        let trimmed = gemini_home.trim();
        if !trimmed.is_empty() {
            let expanded = if let Some(rest) = trimmed.strip_prefix("~/") {
                dirs::home_dir()
                    .ok_or_else(|| "Failed to resolve user home directory".to_string())?
                    .join(rest)
            } else if trimmed == "~" {
                dirs::home_dir()
                    .ok_or_else(|| "Failed to resolve user home directory".to_string())?
            } else {
                std::path::PathBuf::from(trimmed)
            };
            return Ok(expanded.join("antigravity-acp"));
        }
    }

    let home =
        dirs::home_dir().ok_or_else(|| "Failed to resolve user home directory".to_string())?;
    Ok(home.join(".gemini").join("antigravity-acp"))
}

/// Write Antigravity ACP IDE Plugin (`antigravity-acp`) credentials (`acp_token.json`, `settings.json`, and macOS Keychain).
pub fn write_to_acp_credentials(account: &crate::models::Account) -> Result<(), String> {
    let acp_dir = resolve_acp_dir()?;
    if !acp_dir.exists() {
        std::fs::create_dir_all(&acp_dir)
            .map_err(|e| format!("Failed to create antigravity-acp directory: {}", e))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&acp_dir, std::fs::Permissions::from_mode(0o700));
    }

    let token_path = acp_dir.join("acp_token.json");

    // Preserve existing project_id if the account does not carry one and the existing file belongs to the same refresh token
    let existing_project_id = if token_path.exists() {
        std::fs::read_to_string(&token_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                let same_rt = v
                    .get("refresh_token")
                    .and_then(|r| r.as_str())
                    .map(|r| r == account.token.refresh_token)
                    .unwrap_or(false);
                if same_rt {
                    v.get("project_id")
                        .and_then(|p| p.as_str())
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                } else {
                    None
                }
            })
    } else {
        None
    };

    let project_id = account
        .token
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or(existing_project_id)
        .unwrap_or_default();

    #[derive(serde::Serialize)]
    struct AcpCredentialInfo {
        client_id: String,
        client_secret: String,
        refresh_token: String,
        token_uri: String,
        scopes: Vec<String>,
        project_id: String,
    }

    let acp_cred = AcpCredentialInfo {
        client_id: "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com"
            .to_string(),
        client_secret: "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".to_string(),
        refresh_token: account.token.refresh_token.clone(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        scopes: vec![
            "https://www.googleapis.com/auth/cloud-platform".to_string(),
            "https://www.googleapis.com/auth/userinfo.email".to_string(),
            "https://www.googleapis.com/auth/aicode".to_string(),
        ],
        project_id,
    };

    let acp_json = serde_json::to_string(&acp_cred)
        .map_err(|e| format!("Failed to serialize acp_token.json: {}", e))?;

    crate::utils::fs::write_atomic(&token_path, acp_json.as_bytes())
        .map_err(|e| format!("Failed to write acp_token.json: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600));
    }

    // Ensure settings.json has auth.type = "oauth-personal" while preserving existing keys (e.g. gcp)
    let settings_path = acp_dir.join("settings.json");
    let mut settings_obj = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(map) = settings_obj.as_object_mut() {
        let auth_entry = map
            .entry("auth".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !auth_entry.is_object() {
            *auth_entry = serde_json::json!({});
        }
        if let Some(auth_map) = auth_entry.as_object_mut() {
            auth_map.insert(
                "type".to_string(),
                serde_json::Value::String("oauth-personal".to_string()),
            );
        }
    }

    if let Ok(mut settings_str) = serde_json::to_string_pretty(&settings_obj) {
        settings_str.push('\n');
        let _ = crate::utils::fs::write_atomic(&settings_path, settings_str.as_bytes());
    }

    // On macOS, agy_acp_server uses the OS Keychain (service="gemini", account="antigravity-acp")
    // storing the raw JSON string (via ctypes Security framework).
    // In tests with temporary GEMINI_HOME, do not touch the user's real host Keychain.
    #[cfg(target_os = "macos")]
    {
        if std::env::var("GEMINI_HOME").is_err() {
            if let Err(err_msg) =
                write_macos_keychain_generic_password("gemini", "antigravity-acp", &acp_json)
            {
                crate::modules::logger::log_warn(&format!(
                    "[ACP] macOS Keychain update for antigravity-acp returned non-zero ({}); file store at {:?} is updated.",
                    err_msg,
                    token_path
                ));
            }
        }
    }

    crate::modules::logger::log_info(&format!(
        "[ACP] Successfully synced IDE Plugin credentials to {:?} for: {}",
        token_path, account.email
    ));

    Ok(())
}

/// Read the active Antigravity ACP IDE Plugin (`antigravity-acp`) OAuth state (authoritative).
pub fn read_acp_credentials() -> Result<crate::modules::migration::ImportedOAuthState, String> {
    read_acp_credentials_inner(true)
}

/// Passive status detection reader for `antigravity-acp`:
/// skips macOS Keychain probing when the `~/.gemini/antigravity-acp` directory does not exist,
/// prefers the passwordless `acp_token.json` file when present, and otherwise uses single-flight
/// cached Keychain lookup with denial cooldown.
pub(crate) fn read_acp_credentials_passive(
    has_known_acp_account: bool,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    let acp_dir = resolve_acp_dir()?;
    let token_path = acp_dir.join("acp_token.json");
    if token_path.exists() {
        if let Ok(content) = fs::read_to_string(&token_path) {
            if let Ok(state) = parse_keyring_payload(&content) {
                if !state.refresh_token.is_empty() {
                    return Ok(state);
                }
            }
        }
    }

    if !acp_dir.exists() && !has_known_acp_account {
        return Err(format!(
            "Antigravity ACP directory does not exist at {:?}",
            acp_dir
        ));
    }

    read_acp_credentials_inner(false)
}

fn read_acp_credentials_inner(
    bypass_cache: bool,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    // 1. Prefer local file store if present (fast, zero permission popups, isolated)
    let acp_dir = resolve_acp_dir()?;
    let token_path = acp_dir.join("acp_token.json");
    if token_path.exists() {
        if let Ok(content) = fs::read_to_string(&token_path) {
            if let Ok(state) = parse_keyring_payload(&content) {
                if !state.refresh_token.is_empty() {
                    return Ok(state);
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = bypass_cache;

    // 2. Fall back to macOS Keychain if file store is absent/empty and not in test environment
    #[cfg(target_os = "macos")]
    {
        if std::env::var("GEMINI_HOME").is_err() {
            if let KeychainLookupOutcome::Found(secret_str) =
                read_macos_keychain_generic_password("gemini", "antigravity-acp", bypass_cache)
            {
                if let Ok(state) = parse_keyring_payload(&secret_str) {
                    if !state.refresh_token.is_empty() {
                        return Ok(state);
                    }
                }
            }
        }
    }

    if !token_path.exists() {
        return Err(format!(
            "No Antigravity ACP credential file found at {:?}",
            token_path
        ));
    }

    let content = fs::read_to_string(&token_path)
        .map_err(|e| format!("Failed to read acp_token.json: {}", e))?;
    parse_keyring_payload(&content)
}

/// Scan all Antigravity ACP IDE Plugin credential files (`acp_token.json` and backups).
pub fn read_antigravity_acp_credentials() -> Vec<crate::modules::migration::ImportedOAuthState> {
    let mut states = Vec::new();
    let mut seen_tokens = std::collections::HashSet::new();

    if let Ok(primary) = read_acp_credentials() {
        if !primary.refresh_token.is_empty() && seen_tokens.insert(primary.refresh_token.clone()) {
            states.push(primary);
        }
    }

    if let Ok(acp_dir) = resolve_acp_dir() {
        let candidate_files = [
            acp_dir.join("acp_token.json"),
            acp_dir.join("acp_token.json.bak"),
        ];
        for path in &candidate_files {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(state) = parse_keyring_payload(&content) {
                        if !state.refresh_token.is_empty()
                            && seen_tokens.insert(state.refresh_token.clone())
                        {
                            tracing::debug!(
                                "[ACP-Scan] Discovered Antigravity ACP OAuth state in {:?}",
                                path
                            );
                            states.push(state);
                        }
                    }
                }
            }
        }
    }

    states
}

/// Switch the Antigravity ACP IDE Plugin account, verify persisted credentials, and restart running `agy_acp_server` processes.
pub fn switch_acp_account(account: &crate::models::Account) -> Result<usize, String> {
    // 1. Write credentials before killing any running agy_acp_server process
    write_to_acp_credentials(account)?;

    // 2. Terminate running agy_acp_server processes so the host IDE respawns them with new credentials
    let killed = process::kill_acp_server_processes();
    if killed > 0 {
        // Re-assert credentials after terminating processes to prevent any in-flight refresh race from overwriting acp_token.json
        let _ = write_to_acp_credentials(account);
    }

    // 3. Verify stored credentials match
    let stored = read_acp_credentials()?;
    if account.token.refresh_token.is_empty() || stored.refresh_token != account.token.refresh_token
    {
        return Err(
            "Stored ACP IDE Plugin credentials do not match the selected account; the switch was not confirmed."
                .to_string(),
        );
    }

    Ok(killed)
}

/// Headless/Docker 实现：仅执行数据层操作，忽略 UI 和进程控制
pub struct HeadlessIntegration;

impl SystemIntegration for HeadlessIntegration {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        _target_ide: Option<&str>,
    ) -> Result<(), String> {
        if _target_ide == Some("agy") {
            return Err(
                "Switching to the agy CLI is not supported in headless mode (no host keyring access)."
                    .to_string(),
            );
        }

        if is_acp_target(_target_ide) {
            write_to_acp_credentials(account)?;
            crate::modules::logger::log_info(&format!(
                "[Headless] ACP IDE Plugin credentials updated for: {}",
                account.email
            ));
            return Ok(());
        }

        crate::modules::logger::log_info(&format!(
            "[Headless] Account switched in memory: {}",
            account.email
        ));
        // Docker 模式下通常不直接控制宿主机的 VS Code 进程
        // 如果需要同步配置 to 某个 volume，可以在此处添加逻辑
        Ok(())
    }

    fn update_tray(&self) {
        // No-op
    }

    fn show_notification(&self, title: &str, body: &str) {
        crate::modules::logger::log_info(&format!("[Log Notification] {}: {}", title, body));
    }
}

/// CLI 模式实现：支持向宿主操作系统的 Keychain/Keyring 写入凭据（用于终端命令行切号）
pub struct CliIntegration;

impl SystemIntegration for CliIntegration {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        if target_ide == Some("agy") {
            write_to_system_keyring(account)?;
            let _ = write_to_file_credentials(account, true);
            let stored = read_from_system_keyring_only()?;
            verify_agy_credentials(&account.token.refresh_token, &stored.refresh_token)?;

            if let Ok(storage_path) = device::get_storage_path(target_ide) {
                if let Some(ref profile) = account.device_profile {
                    let _ = device::write_profile(&storage_path, profile);
                }
            }
            return Ok(());
        }

        if is_acp_target(target_ide) {
            let _ = switch_acp_account(account)?;
            return Ok(());
        }

        let classic_running = process::is_antigravity_running(None);
        let ide_running = process::is_antigravity_running(Some("ide"));
        let classic_exe = process::get_antigravity_executable_path(Some("classic"));
        let ide_exe = process::get_antigravity_executable_path(Some("ide"));
        let ide_exe_str = ide_exe.as_ref().map(|p| p.to_string_lossy().to_string());

        let (_is_ide, effective_target) = resolve_effective_target(
            target_ide,
            classic_running,
            ide_running,
            classic_exe.is_some(),
            ide_exe_str.as_deref(),
        );

        let _ = write_to_system_keyring(account);
        if let Ok(db_path) = db::get_db_path(effective_target) {
            if db_path.exists() {
                let _ = db::inject_token(
                    &db_path,
                    &account.token.access_token,
                    &account.token.refresh_token,
                    account.token.expiry_timestamp,
                    &account.email,
                    account.token.is_gcp_tos,
                    account.token.project_id.as_deref(),
                    account.token.id_token.as_deref(),
                    account.token.oauth_client_key.as_deref(),
                    effective_target,
                );
            }
        }
        let _ = write_to_file_credentials(account, false);
        Ok(())
    }

    fn update_tray(&self) {}
    fn show_notification(&self, title: &str, body: &str) {
        crate::modules::logger::log_info(&format!("[CLI Notification] {}: {}", title, body));
    }
}

/// 系统集成管理器：替代 Arc<dyn SystemIntegration> 以解决 async trait 的 dyn 兼容性问题
#[derive(Clone)]
pub enum SystemManager {
    Desktop(tauri::AppHandle),
    Headless,
    Cli,
}

impl SystemManager {
    pub async fn on_account_switch(
        &self,
        account: &Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        match self {
            SystemManager::Desktop(handle) => {
                let integration = DesktopIntegration {
                    app_handle: handle.clone(),
                };
                integration.on_account_switch(account, target_ide).await
            }
            SystemManager::Headless => {
                let integration = HeadlessIntegration;
                integration.on_account_switch(account, target_ide).await
            }
            SystemManager::Cli => {
                let integration = CliIntegration;
                integration.on_account_switch(account, target_ide).await
            }
        }
    }

    pub fn update_tray(&self) {
        if let SystemManager::Desktop(handle) = self {
            let integration = DesktopIntegration {
                app_handle: handle.clone(),
            };
            integration.update_tray();
        }
    }

    pub fn show_notification(&self, title: &str, body: &str) {
        match self {
            SystemManager::Desktop(handle) => {
                let integration = DesktopIntegration {
                    app_handle: handle.clone(),
                };
                integration.show_notification(title, body);
            }
            SystemManager::Headless => {
                let integration = HeadlessIntegration;
                integration.show_notification(title, body);
            }
            SystemManager::Cli => {
                let integration = CliIntegration;
                integration.show_notification(title, body);
            }
        }
    }

    pub fn handles_wsl_sync(&self, target_ide: Option<&str>) -> bool {
        match self {
            SystemManager::Desktop(_) => desktop_handles_wsl_sync(target_ide),
            SystemManager::Headless | SystemManager::Cli => false,
        }
    }
}

impl SystemIntegration for SystemManager {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        self.on_account_switch(account, target_ide).await
    }

    fn update_tray(&self) {
        self.update_tray();
    }

    fn show_notification(&self, title: &str, body: &str) {
        self.show_notification(title, body);
    }

    fn handles_wsl_sync(&self, target_ide: Option<&str>) -> bool {
        self.handles_wsl_sync(target_ide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_credentials_require_an_exact_nonempty_readback() {
        assert!(verify_agy_credentials("selected-token", "selected-token").is_ok());
        assert!(verify_agy_credentials("", "").is_err());
        let error = verify_agy_credentials("selected-token", "other-token").unwrap_err();
        assert!(!error.contains("selected-token"));
        assert!(!error.contains("other-token"));
    }

    #[test]
    fn test_parse_keyring_payload_nested_token() {
        let payload = r#"{
            "token": {
                "access_token": "ya29.test",
                "token_type": "Bearer",
                "refresh_token": "1//test_refresh_token_123",
                "expiry": "2026-09-19T10:00:00.000000Z"
            },
            "auth_method": "consumer"
        }"#;
        let state = parse_keyring_payload(payload).expect("Failed to parse nested keyring payload");
        assert_eq!(state.refresh_token, "1//test_refresh_token_123");
        assert!(state.is_gcp_tos);
    }

    #[test]
    fn test_parse_keyring_payload_flat_token() {
        let payload = r#"{
            "access_token": "ya29.test",
            "refresh_token": "1//test_refresh_token_flat"
        }"#;
        let state = parse_keyring_payload(payload).expect("Failed to parse flat keyring payload");
        assert_eq!(state.refresh_token, "1//test_refresh_token_flat");
    }

    #[test]
    fn test_parse_keyring_payload_missing_token() {
        let payload = r#"{ "auth_method": "consumer" }"#;
        let res = parse_keyring_payload(payload);
        assert!(res.is_err());
    }

    #[test]
    fn test_resolve_effective_target_explicit_classic() {
        // 显式指定 classic，即便 IDE 正在运行或只有 IDE exe，也必须严格判定为经典版
        let (is_ide, effective) = resolve_effective_target(
            Some("classic"),
            false,
            true,
            false,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, Some("classic"));
    }

    #[test]
    fn test_resolve_effective_target_explicit_ide() {
        // 显式指定 ide，必须判定为 ide
        let (is_ide, effective) = resolve_effective_target(Some("ide"), true, false, true, None);
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));
    }

    #[test]
    fn test_resolve_effective_target_autodetect_classic_running() {
        // target_ide 为 None，经典版正在运行，必须优先保持经典版
        let (is_ide, effective) = resolve_effective_target(
            None,
            true,
            true,
            true,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, None);
    }

    #[test]
    fn test_resolve_effective_target_autodetect_ide_running_only() {
        // target_ide 为 None，仅 IDE 正在运行，推导为 IDE
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            true,
            true,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));
    }

    #[test]
    fn test_resolve_effective_target_autodetect_classic_exe_exists() {
        // target_ide 为 None，两者均未运行，但经典版 exe 存在，优先经典版
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            false,
            true,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, None);
    }

    #[test]
    fn test_resolve_effective_target_autodetect_fallback_ide_exe() {
        // target_ide 为 None，两者均未运行，无经典版但有 IDE exe，推导为 IDE
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));

        // 测试下划线命名 antigravity_ide
        let (is_ide_underscore, effective_underscore) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some("/usr/local/bin/antigravity_ide"),
        );
        assert!(is_ide_underscore);
        assert_eq!(effective_underscore, Some("ide"));
    }

    #[test]
    fn test_resolve_effective_target_autodetect_default_fallback() {
        // target_ide 为 None，均未运行且均未检测到 exe，默认保底经典版
        let (is_ide, effective) = resolve_effective_target(None, false, false, false, None);
        assert!(!is_ide);
        assert_eq!(effective, None);
    }

    #[test]
    fn test_resolve_effective_target_detects_windows_ide_260_install_path() {
        // [FIX #3762] Windows 2.6.0 IDE 安装在 AppData\Local\Programs\Antigravity IDE，
        // 可执行文件名仍为 Antigravity.exe，必须靠安装目录的 IDE 命名推导为 IDE 目标。
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some(r"C:\Users\user\AppData\Local\Programs\Antigravity IDE\Antigravity.exe"),
        );
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));

        // 经典版同路径不得被推导为 IDE
        let (is_ide_classic, effective_classic) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some(r"C:\Users\user\AppData\Local\Programs\Antigravity\Antigravity.exe"),
        );
        assert!(!is_ide_classic);
        assert_eq!(effective_classic, None);
    }

    #[test]
    fn test_classic_credential_file_sync_writes_jetski_and_oauth_creds() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path().join(".gemini");

        let token = crate::models::TokenData::new(
            "ya29.classic_access_token".to_string(),
            "1//classic_refresh_token".to_string(),
            3600,
            Some("classic-user@example.com".to_string()),
            Some("proj-classic".to_string()),
            None,
            true,
            Some("id-token-classic".to_string()),
        );
        let account = crate::models::Account::new(
            "classic-acc-1".to_string(),
            "classic-user@example.com".to_string(),
            token,
        );

        // 模拟经典款切号写入凭据 (sync_agy_cli = false)
        let res = write_to_file_credentials_to_dir(&gemini_dir, &account, false);
        assert!(res.is_ok());

        // 1. 验证 jetski-standalone-oauth-token 写入成功且内容正确 (Fixes #3829)
        let standalone_path = gemini_dir.join("jetski-standalone-oauth-token");
        assert!(standalone_path.exists());
        let standalone_content = std::fs::read_to_string(&standalone_path).unwrap();
        let standalone_json: serde_json::Value = serde_json::from_str(&standalone_content).unwrap();
        assert_eq!(
            standalone_json["token"]["access_token"],
            "ya29.classic_access_token"
        );
        assert_eq!(
            standalone_json["token"]["refresh_token"],
            "1//classic_refresh_token"
        );
        assert_eq!(standalone_json["token"]["token_type"], "Bearer");
        assert_eq!(standalone_json["auth_method"], "consumer");
        assert_eq!(
            standalone_json["id_token_email"],
            "classic-user@example.com"
        );
        assert_eq!(standalone_json["id_token"], "id-token-classic");

        // 2. 验证 read_from_file_credentials_from_dir 能够以此文件作为原生回退读取
        let readback = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(readback.refresh_token, "1//classic_refresh_token");

        // 3. 验证 oauth_creds.json 与 google_accounts.json 正确生成
        let oauth_path = gemini_dir.join("oauth_creds.json");
        assert!(oauth_path.exists());
        let oauth_content = std::fs::read_to_string(&oauth_path).unwrap();
        let oauth_json: serde_json::Value = serde_json::from_str(&oauth_content).unwrap();
        assert_eq!(oauth_json["refresh_token"], "1//classic_refresh_token");

        let accounts_path = gemini_dir.join("google_accounts.json");
        assert!(accounts_path.exists());
        let accounts_content = std::fs::read_to_string(&accounts_path).unwrap();
        let accounts_json: serde_json::Value = serde_json::from_str(&accounts_content).unwrap();
        assert_eq!(accounts_json["active"], "classic-user@example.com");

        // 4. 验证严格隔离：sync_agy_cli = false 时不得向 antigravity-cli 目录写入凭据
        let agy_cli_token_path = gemini_dir
            .join("antigravity-cli")
            .join("antigravity-oauth-token");
        assert!(
            !agy_cli_token_path.exists(),
            "Classic switch must NOT write antigravity-cli credentials"
        );
    }

    #[test]
    fn test_agy_cli_credential_sync_writes_dedicated_cli_token() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path().join(".gemini");

        let token = crate::models::TokenData::new(
            "ya29.agy_access_token".to_string(),
            "1//agy_refresh_token".to_string(),
            3600,
            Some("agy-user@example.com".to_string()),
            None,
            None,
            true,
            None,
        );
        let account = crate::models::Account::new(
            "agy-acc-1".to_string(),
            "agy-user@example.com".to_string(),
            token,
        );

        // 切换 agy CLI 时同步写入 (sync_agy_cli = true)
        let res = write_to_file_credentials_to_dir(&gemini_dir, &account, true);
        assert!(res.is_ok());

        let agy_cli_token_path = gemini_dir
            .join("antigravity-cli")
            .join("antigravity-oauth-token");
        assert!(agy_cli_token_path.exists());
        let content = std::fs::read_to_string(&agy_cli_token_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["token"]["refresh_token"], "1//agy_refresh_token");
    }

    #[test]
    fn test_classic_sqlite_dual_write_preserves_and_updates_state_db() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("state.vscdb");

        // 初始化模拟 SQLite 数据库，包含初始旧数据
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('antigravityAuthStatus', 'old_auth_data')",
                [],
            )
            .unwrap();
        }

        let backup_path = db_path.with_extension("vscdb.backup");
        let _ = std::fs::copy(&db_path, &backup_path);
        assert!(backup_path.exists());

        // 注入新凭证与机器 ID
        let inject_res = crate::modules::db::inject_token(
            &db_path,
            "ya29.new_classic_token",
            "1//new_classic_refresh",
            1893456000,
            "new-classic@example.com",
            true,
            None,
            None,
            None,
            Some("classic"),
        );
        assert!(inject_res.is_ok());

        let machine_res =
            crate::modules::db::write_service_machine_id(&db_path, "test-mac-machine-id");
        assert!(machine_res.is_ok());

        // 验证主数据库已正确写入新 Token、用户状态、引导状态与机器指纹
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let oauth_token_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(oauth_token_count, 1);

        let user_status_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.userStatus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(user_status_count, 1);

        let onboarding_val: String = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'antigravityOnboarding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(onboarding_val, "true");

        let machine_val: String = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'telemetry.serviceMachineId'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(machine_val, "test-mac-machine-id");

        // 验证备份数据库保留了原始内容且未被污染
        let backup_conn = rusqlite::Connection::open(&backup_path).unwrap();
        let backup_oauth_count: i64 = backup_conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            backup_oauth_count, 0,
            "Backup DB must not contain newly injected oauthToken"
        );

        let backup_machine_count: i64 = backup_conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'telemetry.serviceMachineId'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            backup_machine_count, 0,
            "Backup DB must not contain newly injected machine ID"
        );

        let backup_old_auth: String = backup_conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'antigravityAuthStatus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(backup_old_auth, "old_auth_data");
    }

    #[test]
    fn test_read_from_file_credentials_priority_hierarchy() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path().join(".gemini");
        std::fs::create_dir_all(gemini_dir.join("antigravity-cli")).unwrap();

        let agy_account = crate::models::Account::new(
            "agy-1".to_string(),
            "agy@example.com".to_string(),
            crate::models::TokenData::new(
                "ya29.agy_acc".to_string(),
                "1//agy_refresh".to_string(),
                3600,
                Some("agy@example.com".to_string()),
                None,
                None,
                true,
                None,
            ),
        );
        let standalone_account = crate::models::Account::new(
            "standalone-1".to_string(),
            "standalone@example.com".to_string(),
            crate::models::TokenData::new(
                "ya29.standalone_acc".to_string(),
                "1//standalone_refresh".to_string(),
                3600,
                Some("standalone@example.com".to_string()),
                None,
                None,
                true,
                None,
            ),
        );

        // 1. 同时写入 CLI 凭据与 Standalone 凭据，验证 CLI 凭据优先级最高
        let _ = write_to_file_credentials_to_dir(&gemini_dir, &agy_account, true);
        // 覆盖 standalone 凭据为不同值
        let _ = write_to_file_credentials_to_dir(&gemini_dir, &standalone_account, false);
        // 此时 antigravity-cli/antigravity-oauth-token 为 agy_refresh，jetski-standalone-oauth-token 为 standalone_refresh
        let cred = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(
            cred.refresh_token, "1//agy_refresh",
            "CLI credentials must have highest priority over standalone"
        );

        // 2. 移除 CLI 凭据后，验证 jetski-standalone-oauth-token 优于 oauth_creds.json
        std::fs::remove_file(
            gemini_dir
                .join("antigravity-cli")
                .join("antigravity-oauth-token"),
        )
        .unwrap();
        let cred_standalone = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(
            cred_standalone.refresh_token, "1//standalone_refresh",
            "Standalone oauth token must take priority over oauth_creds.json"
        );

        // 3. 移除 standalone 凭据后，验证回退到 oauth_creds.json
        std::fs::remove_file(gemini_dir.join("jetski-standalone-oauth-token")).unwrap();
        let cred_oauth = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(
            cred_oauth.refresh_token, "1//standalone_refresh",
            "Should fallback to oauth_creds.json when standalone token missing"
        );

        // 4. 全部移除后，验证返回明确的错误
        std::fs::remove_file(gemini_dir.join("oauth_creds.json")).unwrap();
        let cred_err = read_from_file_credentials_from_dir(&gemini_dir);
        assert!(
            cred_err.is_err(),
            "Must return error when no credential files exist"
        );
    }

    #[test]
    fn test_classic_sqlite_injection_failure_handling() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let corrupt_db_path = temp_dir.path().join("corrupted_state.vscdb");

        // 写入损坏的非 SQLite 文件
        std::fs::write(&corrupt_db_path, b"not a valid sqlite database header").unwrap();

        let inject_res = crate::modules::db::inject_token(
            &corrupt_db_path,
            "ya29.token",
            "1//refresh",
            1893456000,
            "corrupt@example.com",
            true,
            None,
            None,
            None,
            Some("classic"),
        );

        // 必须优雅返回 Err 而不是 panic
        assert!(
            inject_res.is_err(),
            "Injecting token into corrupted DB must return Err gracefully"
        );
    }

    #[test]
    fn test_is_acp_target_aliases() {
        assert!(is_acp_target(Some("acp")));
        assert!(is_acp_target(Some("plugin")));
        assert!(is_acp_target(Some("ide-plugin")));
        assert!(!is_acp_target(Some("ide")));
        assert!(!is_acp_target(Some("agy")));
        assert!(!is_acp_target(Some("classic")));
        assert!(!is_acp_target(None));
    }

    struct KeychainBackupGuard {
        #[cfg(target_os = "macos")]
        service: &'static str,
        #[cfg(target_os = "macos")]
        account: &'static str,
        #[cfg(target_os = "macos")]
        original_val: Option<String>,
    }

    impl KeychainBackupGuard {
        fn new(service: &'static str, account: &'static str) -> Self {
            #[cfg(target_os = "macos")]
            {
                if std::env::var("GEMINI_HOME").is_ok() {
                    return Self {
                        service,
                        account,
                        original_val: None,
                    };
                }
                let original_val =
                    match read_macos_keychain_generic_password(service, account, true) {
                        KeychainLookupOutcome::Found(val) => Some(val),
                        _ => None,
                    };
                Self {
                    service,
                    account,
                    original_val,
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (service, account);
                Self {}
            }
        }
    }

    impl Drop for KeychainBackupGuard {
        fn drop(&mut self) {
            #[cfg(target_os = "macos")]
            {
                if std::env::var("GEMINI_HOME").is_ok() {
                    return;
                }
                if let Some(val) = &self.original_val {
                    let _ = write_macos_keychain_generic_password(self.service, self.account, val);
                } else {
                    let _ = Command::new("security")
                        .args([
                            "delete-generic-password",
                            "-s",
                            self.service,
                            "-a",
                            self.account,
                        ])
                        .output();
                }
                invalidate_system_keychain_cache(self.service, self.account);
            }
        }
    }

    #[test]
    fn test_write_and_read_acp_credentials() {
        let _env_guard = crate::modules::account::SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp_dir = std::env::temp_dir().join(format!("agy_acp_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).expect("create temp dir");
        std::env::set_var("GEMINI_HOME", &tmp_dir);
        let _keychain_guard = KeychainBackupGuard::new("gemini", "antigravity-acp");

        let token = crate::models::TokenData::new(
            "ya29.test_access_token".to_string(),
            "1//test_acp_refresh_token_999".to_string(),
            3600,
            Some("acp-user@example.com".to_string()),
            Some("test-gcp-project-id".to_string()),
            None,
            true,
            None,
        );
        let account = crate::models::Account::new(
            "acc-acp-1".to_string(),
            "acp-user@example.com".to_string(),
            token,
        );

        write_to_acp_credentials(&account).expect("write_to_acp_credentials should succeed");

        let acp_dir = tmp_dir.join("antigravity-acp");
        let token_path = acp_dir.join("acp_token.json");
        let settings_path = acp_dir.join("settings.json");
        assert!(token_path.exists(), "acp_token.json should exist");
        assert!(settings_path.exists(), "settings.json should exist");

        let settings_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&settings_path).expect("read settings.json"),
        )
        .expect("parse settings.json");
        assert_eq!(
            settings_json
                .get("auth")
                .and_then(|a| a.get("type"))
                .and_then(|t| t.as_str()),
            Some("oauth-personal")
        );

        let state = read_acp_credentials().expect("read_acp_credentials should succeed");
        assert_eq!(state.refresh_token, "1//test_acp_refresh_token_999");
        assert_eq!(state.project_id.as_deref(), Some("test-gcp-project-id"));

        let scanned = read_antigravity_acp_credentials();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].refresh_token, "1//test_acp_refresh_token_999");

        std::env::remove_var("GEMINI_HOME");
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_resolve_account_summary_in_index_acp_round_robin() {
        let index = crate::models::AccountIndex {
            version: "2.0".to_string(),
            accounts: vec![
                crate::models::AccountSummary {
                    id: "id-1".to_string(),
                    email: "user1@example.com".to_string(),
                    name: None,
                    disabled: false,
                    proxy_disabled: false,
                    protected_models: std::collections::HashSet::new(),
                    created_at: 0,
                    last_used: 0,
                },
                crate::models::AccountSummary {
                    id: "id-2".to_string(),
                    email: "user2@example.com".to_string(),
                    name: None,
                    disabled: false,
                    proxy_disabled: false,
                    protected_models: std::collections::HashSet::new(),
                    created_at: 0,
                    last_used: 0,
                },
            ],
            current_account_id: Some("id-1".to_string()),
            current_target_ide: Some("acp".to_string()),
            current_classic_account_id: Some("id-1".to_string()),
            current_ide_account_id: None,
            current_agy_account_id: None,
            current_acp_account_id: Some("id-2".to_string()),
        };

        let next_for_acp =
            crate::modules::account::resolve_account_summary_in_index(&index, None, Some("acp"))
                .expect("resolve next for acp");
        assert_eq!(next_for_acp.id, "id-1");

        let next_for_plugin_alias = crate::modules::account::resolve_account_summary_in_index(
            &index,
            None,
            Some("ide-plugin"),
        )
        .expect("resolve next for ide-plugin");
        assert_eq!(next_for_plugin_alias.id, "id-1");
    }

    #[test]
    fn test_classify_macos_security_failure_distinguishes_not_found_and_denied() {
        assert_eq!(
            classify_macos_security_failure(
                Some(44),
                "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain."
            ),
            KeychainLookupOutcome::NotFound
        );
        assert!(matches!(
            classify_macos_security_failure(
                Some(128),
                "security: SecKeychainSearchCopyNext: User canceled the operation."
            ),
            KeychainLookupOutcome::Denied(_)
        ));
        assert!(matches!(
            classify_macos_security_failure(
                Some(36),
                "security: SecKeychainSearchCopyNext: User interaction is not allowed."
            ),
            KeychainLookupOutcome::Denied(_)
        ));
        assert!(matches!(
            classify_macos_security_failure(Some(51), "Authentication failed."),
            KeychainLookupOutcome::Denied(_)
        ));
    }

    #[test]
    fn test_keychain_read_coordinator_caches_coalesces_cooldowns_and_invalidates() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let coordinator = KeychainReadCoordinator::new(Duration::from_secs(60), None);
        let calls = AtomicUsize::new(0);

        // First call populates the cache
        let first = coordinator.read_with("gemini", "antigravity", false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Found("token-v1".to_string())
        });
        assert_eq!(first, KeychainLookupOutcome::Found("token-v1".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Second passive read hits cache without re-invoking security
        let second = coordinator.read_with("gemini", "antigravity", false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Found("token-v2".to_string())
        });
        assert_eq!(second, KeychainLookupOutcome::Found("token-v1".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Denial stays suppressed for passive readers until explicit invalidate() or bypass_cache
        coordinator.invalidate("gemini", "antigravity");
        let denied = coordinator.read_with("gemini", "antigravity", false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Denied("User canceled".to_string())
        });
        assert!(matches!(denied, KeychainLookupOutcome::Denied(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let during_suppression = coordinator.read_with("gemini", "antigravity", false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Found("should-not-run".to_string())
        });
        assert!(matches!(
            during_suppression,
            KeychainLookupOutcome::Denied(_)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // Explicit bypass_cache=true still probes and updates the slot even when sticky
        let forced = coordinator.read_with("gemini", "antigravity", true, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Found("token-v2".to_string())
        });
        assert_eq!(forced, KeychainLookupOutcome::Found("token-v2".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // Active write invalidation clears prior state
        coordinator.invalidate("gemini", "antigravity");
        let refreshed = coordinator.read_with("gemini", "antigravity", true, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Found("token-v3".to_string())
        });
        assert_eq!(
            refreshed,
            KeychainLookupOutcome::Found("token-v3".to_string())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);

        // Finite cooldown mode suppresses during TTL=0? Verify finite Duration(0) allows retry
        let finite =
            KeychainReadCoordinator::new(Duration::from_secs(60), Some(Duration::from_secs(0)));
        let _ = finite.read_with("gemini", "antigravity", false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Denied("transient".to_string())
        });
        let retried = finite.read_with("gemini", "antigravity", false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            KeychainLookupOutcome::Found("ok".to_string())
        });
        assert_eq!(retried, KeychainLookupOutcome::Found("ok".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn test_switch_acp_account_e2e_flow_and_project_id_preservation() {
        let _env_guard = crate::modules::account::SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp_dir = std::env::temp_dir().join(format!("agy_acp_e2e_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).expect("create temp dir");
        std::env::set_var("GEMINI_HOME", &tmp_dir);
        let _keychain_guard = KeychainBackupGuard::new("gemini", "antigravity-acp");

        let token1 = crate::models::TokenData::new(
            "ya29.test_acp_token_1".to_string(),
            "1//test_acp_rt_1".to_string(),
            3600,
            Some("user1@example.com".to_string()),
            Some("project-alpha-123".to_string()),
            None,
            true,
            None,
        );
        let account1 = crate::models::Account::new(
            "acc-1".to_string(),
            "user1@example.com".to_string(),
            token1,
        );

        // 1. 切换到 account1
        let switch_res1 = switch_acp_account(&account1);
        assert!(switch_res1.is_ok(), "First switch must succeed");

        let state1 = read_acp_credentials().expect("Must read account1 credentials");
        assert_eq!(state1.refresh_token, "1//test_acp_rt_1");
        assert_eq!(state1.project_id.as_deref(), Some("project-alpha-123"));

        // 验证物理文件包含 aicode scope
        let token_file = tmp_dir.join("antigravity-acp").join("acp_token.json");
        assert!(token_file.exists());
        let file_content = std::fs::read_to_string(&token_file).unwrap();
        let json_val: serde_json::Value = serde_json::from_str(&file_content).unwrap();
        let scopes = json_val["scopes"]
            .as_array()
            .expect("scopes must be an array");
        assert!(
            scopes.iter().any(|s| s.as_str() == Some("https://www.googleapis.com/auth/aicode")),
            "acp_token.json must strictly contain https://www.googleapis.com/auth/aicode scope for agy_acp_server"
        );

        // 2. 模拟 account1 凭证更新（无显式 project_id 时，必须继承同 refresh_token 已有的 project_id）
        let token1_no_proj = crate::models::TokenData::new(
            "ya29.test_acp_token_1_refreshed".to_string(),
            "1//test_acp_rt_1".to_string(),
            3600,
            Some("user1@example.com".to_string()),
            None, // 没有显式 project_id
            None,
            true,
            None,
        );
        let account1_refreshed = crate::models::Account::new(
            "acc-1".to_string(),
            "user1@example.com".to_string(),
            token1_no_proj,
        );
        write_to_acp_credentials(&account1_refreshed).expect("credential re-write must succeed");
        let state1_preserved = read_acp_credentials().expect("Must read refreshed credentials");
        assert_eq!(
            state1_preserved.project_id.as_deref(),
            Some("project-alpha-123"),
            "Project ID must be preserved when existing file has the same refresh token"
        );

        // 3. 切换到全新 account2（带不同 project_id）
        let token2 = crate::models::TokenData::new(
            "ya29.test_acp_token_2".to_string(),
            "1//test_acp_rt_2".to_string(),
            3600,
            Some("user2@example.com".to_string()),
            Some("project-beta-456".to_string()),
            None,
            true,
            None,
        );
        let account2 = crate::models::Account::new(
            "acc-2".to_string(),
            "user2@example.com".to_string(),
            token2,
        );
        let switch_res2 = switch_acp_account(&account2);
        assert!(switch_res2.is_ok(), "Second switch must succeed");

        let state2 = read_acp_credentials().expect("Must read account2 credentials");
        assert_eq!(state2.refresh_token, "1//test_acp_rt_2");
        assert_eq!(state2.project_id.as_deref(), Some("project-beta-456"));

        std::env::remove_var("GEMINI_HOME");
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn system_integration_handles_wsl_sync_dispatch_and_desktop_targets() {
        fn check_handles(
            integration: &(impl SystemIntegration + ?Sized),
            target_ide: Option<&str>,
        ) -> bool {
            integration.handles_wsl_sync(target_ide)
        }

        assert!(!check_handles(&HeadlessIntegration, None));
        assert!(!check_handles(&HeadlessIntegration, Some("ide")));
        assert!(!check_handles(&CliIntegration, None));
        assert!(!check_handles(&CliIntegration, Some("agy")));
        assert!(!check_handles(&SystemManager::Headless, None));
        assert!(!check_handles(&SystemManager::Cli, Some("ide")));

        assert!(desktop_handles_wsl_sync(None));
        assert!(desktop_handles_wsl_sync(Some("ide")));
        assert!(desktop_handles_wsl_sync(Some("classic")));
        assert!(!desktop_handles_wsl_sync(Some("agy")));
        assert!(!desktop_handles_wsl_sync(Some("acp")));
        assert!(!desktop_handles_wsl_sync(Some("plugin")));
        assert!(!desktop_handles_wsl_sync(Some("ide-plugin")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_keychain_large_payload_over_4kb_roundtrip() {
        let service = format!("antigravity-test-large-payload-{}", uuid::Uuid::new_v4());
        let account = "large-oauth-blob";
        let payload = format!("go-keyring-base64:{}", "A".repeat(10_000));

        match write_macos_keychain_generic_password(&service, account, &payload) {
            Ok(()) => {
                let outcome = read_macos_keychain_generic_password(&service, account, true);
                macos_keychain_ffi::delete_generic_password(&service, account);
                assert_eq!(outcome, KeychainLookupOutcome::Found(payload));
            }
            Err(e) => {
                // In non-interactive test environments where the default keychain is locked or
                // requires authorization for ad-hoc test binaries, ScopedNoKeychainUi must
                // cleanly suppress the prompt without modal dialogs or deadlocks.
                assert!(
                    e.contains("-25293")
                        || e.contains("-25308")
                        || e.contains("-128")
                        || e.contains("failed"),
                    "Unexpected Keychain write failure: {}",
                    e
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_keychain_apple_tool_and_sub_4kb_roundtrip_without_ui_prompt() {
        let service = format!("antigravity-test-sub4k-{}", uuid::Uuid::new_v4());
        let account = "agy-oauth-token";
        let payload = format!("go-keyring-base64:{}", "B".repeat(2_400));

        match write_macos_keychain_generic_password(&service, account, &payload) {
            Ok(()) => {
                let cli_read = Command::new("security")
                    .args(["find-generic-password", "-s", &service, "-a", account, "-w"])
                    .output()
                    .expect("security find-generic-password should run");
                assert!(
                    cli_read.status.success(),
                    "security CLI should read item without prompting: stderr={}",
                    String::from_utf8_lossy(&cli_read.stderr)
                );
                assert_eq!(String::from_utf8_lossy(&cli_read.stdout).trim(), payload);

                let app_read = read_macos_keychain_generic_password(&service, account, true);
                let _ = Command::new("security")
                    .args(["delete-generic-password", "-s", &service, "-a", account])
                    .output();
                macos_keychain_ffi::delete_generic_password(&service, account);

                assert_eq!(app_read, KeychainLookupOutcome::Found(payload));
            }
            Err(e) => {
                assert!(
                    e.contains("-25293")
                        || e.contains("-25308")
                        || e.contains("-128")
                        || e.contains("failed"),
                    "Unexpected Keychain write failure: {}",
                    e
                );
            }
        }
    }

    #[test]
    fn test_classify_linux_secret_tool_failure_distinguishes_not_found_warnings_from_denied() {
        assert_eq!(
            classify_linux_secret_tool_failure(Some(1), ""),
            KeychainLookupOutcome::NotFound
        );
        assert_eq!(
            classify_linux_secret_tool_failure(
                Some(1),
                "(secret-tool:4210): dbind-WARNING **: 03:10:00.000: Couldn't connect to accessibility bus\nGtk-WARNING **: cannot open display"
            ),
            KeychainLookupOutcome::NotFound
        );
        assert_eq!(
            classify_linux_secret_tool_failure(Some(1), "secret-tool: Item not found"),
            KeychainLookupOutcome::NotFound
        );
        assert!(matches!(
            classify_linux_secret_tool_failure(
                Some(1),
                "secret-tool: Cannot unlock keyring: user cancelled prompt"
            ),
            KeychainLookupOutcome::Denied(_)
        ));
        assert!(matches!(
            classify_linux_secret_tool_failure(
                Some(1),
                "The name org.freedesktop.secrets was not provided by any .service files"
            ),
            KeychainLookupOutcome::Denied(_)
        ));
        assert!(matches!(
            classify_linux_secret_tool_failure(
                Some(1),
                "secret-tool: Cannot autolaunch D-Bus without X11 $DISPLAY"
            ),
            KeychainLookupOutcome::Denied(_)
        ));
    }
}

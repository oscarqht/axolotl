# Website passkeys on macOS

Axolotl leaves `navigator.credentials.create()` and `navigator.credentials.get()`
to WKWebView. WebKit enforces secure contexts, origins and relying-party rules;
macOS and the selected credential provider handle credential storage and user
verification. There is no injected credential bridge, private-key store or
Axolotl passkey manager. Windows behavior is unchanged.

## Requirements and signing

The browser permission API requires macOS 13.3 or later. Apple requires the
restricted `com.apple.developer.web-browser.public-key-credential` entitlement
to access passkeys for arbitrary websites. An organization Apple Developer
Account Holder must request approval through Apple's macOS Browsers Passkeys
process, then create a matching App ID, signing certificate and provisioning
profile with the capability enabled. Adding a plist key alone does not grant
access. The credential provider also needs to be configured and available.

Default `cargo run` and `scripts/package-macos.sh` builds do not have this
capability. They remain usable and show an explanatory message from
**Axolotl → Enable Passkeys…** without invoking the restricted API.

After Apple approval, export a signing entitlements plist for your App ID and
profile, including the boolean browser entitlement shown in
`scripts/macos-passkeys.entitlements`. Include any application/team identifier
entitlements required by your profile. Keep certificates and profiles outside
the repository. Use absolute paths:

```sh
AXOLOTL_SIGNING_IDENTITY='Developer ID Application: Your Organization (TEAMID)' \
AXOLOTL_BUNDLE_ID='your.approved.bundle.id' \
AXOLOTL_ENTITLEMENTS='/absolute/path/to/signing.entitlements' \
AXOLOTL_PROVISIONING_PROFILE='/absolute/path/to/profile.provisionprofile' \
scripts/package-macos.sh release
codesign --verify --strict target/release/Axolotl.app
codesign -d --entitlements :- target/release/Axolotl.app
```

The script embeds the supplied profile and signs with hardened runtime and a
timestamp. It does not obtain Apple's approval, validate the profile's grant,
or notarize the app. Confirm the profile is unexpired and matches the certificate,
bundle ID and requested entitlements before testing or distribution. Code signature
verification alone does not prove the OS will grant passkey access. Notarization
and distribution are separate release steps.

## Permission behavior

Choose **Enable Passkeys…** with a browser window open. Already-authorized access
shows confirmation; undetermined access opens Apple's permission dialog. Denied,
cancelled, unsupported and unknown states show guidance without assuming access.
There is only one app-level permission request at a time. The foreground remains
responsive; if no callback arrives within 60 seconds the action reports a timeout
and releases its pending state, even if the requesting window has closed.
Dismiss any outstanding system dialog before retrying. This timeout does not
cancel Apple's dialog. Permission can be changed in the macOS Passwords settings
(location varies by OS version).

Website-level cancellation and unavailable-provider errors are handled by WebKit
and returned to the website through the standard WebAuthn promise. Granting
browser permission does not guarantee a successful website sign-in.

## Manual acceptance checks

Use an approved, provisioned build, a configured provider and a test account on
an HTTPS relying party you control or a WebAuthn test site. Keep the same bundle
ID/signing identity while checking permission persistence.

1. Choose Enable Passkeys, allow access, and repeat to confirm authorized status.
2. Register a passkey from the website using user verification; sign out, then
   sign in with it. Restart the app and repeat sign-in. Confirm the server
   validates the registration and assertion for its expected origin and RP ID.
3. Cancel both registration and sign-in sheets. Confirm the website receives a
   rejection, stays responsive and can retry or use another sign-in method.
4. Deny browser permission and retry. Change access in system settings and
   confirm the app observes it. Test with no available credential/provider.
5. Close the requesting window during permission authorization, complete or
   dismiss the system dialog, and verify a subsequent window can request again.
6. Confirm invalid RP IDs and insecure origins cannot create/use credentials.
7. On an ad-hoc build, confirm Enable Passkeys explains missing signing; on an
   older OS, confirm the unsupported message and ordinary browsing still work.

Unit tests cover OS/entitlement gating, authorization-state mapping and unknown
states. They do not perform credential ceremonies or simulate OS authorization.
Those require the signed build and interactive checks above.

## Apple references

- [Passkey use in web browsers](https://developer.apple.com/documentation/authenticationservices/passkey-use-in-web-browsers)
- [Browser passkey entitlement and approval](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.web-browser.public-key-credential)
- [Browser credential manager](https://developer.apple.com/documentation/authenticationservices/asauthorizationwebbrowserpublickeycredentialmanager)

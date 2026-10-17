# Update signing procedure

This procedure covers the minisign key that signs update manifests. Nothing in this repository generates, stores or provisions a signing secret. The client embeds only the public key.

## Separation of secrets

- The signing secret key never enters client application state, exports, backups, logs or snapshots.
- The client download credential is a different secret: a fine-grained GitHub token with Contents read access to the single release repository. It is entered by the user and held in the operating system secure store or, if the user chooses, in memory for one session.
- Release automation holds the signing key only as a protected GitHub Actions secret on a protected environment that requires manual approval. Pull requests and forks never receive it.

## Key generation and custody

1. On an offline machine, generate a minisign key pair with a passphrase of at least 20 random characters.
2. Record the public key in `src/update/verifier.rs` as the embedded key and in `tests/update/fixtures` only for test keys. Production and test keys must differ.
3. Store an encrypted copy of the secret key and its passphrase on two disconnected removable media kept in separate locations.
4. Rehearse restore twice a year: decrypt a copy on an offline machine, sign a throwaway file, verify it with the embedded public key, then securely erase the working copy.

## Public-key transition (over-the-air)

A new key never replaces the old one by trust in unsigned data.

1. Release N is signed by the old key and embeds both the old and the new public key in the application.
2. Release N+1 and later are signed by the new key. A client that has not installed release N cannot verify release N+1 and must install release N or a later full package manually.
3. After the supported upgrade window, release N+2 drops the old key.

## Compromise response

1. Revoke the GitHub secret and rotate the environment protection.
2. Publish a new release signed by the next key only after an out-of-band announcement; clients need a manual install of the fixed package.
3. Never reuse a version number for different bytes.

## Release checklist

- The Cargo version, package version and signed manifest version are the same `MAJOR.MINOR.PATCH`.
- `manifest.json` and `manifest.json.minisig` are uploaded in the same release as the packages.
- The release is created as a draft, every asset is verified against the manifest, and only then is it published.

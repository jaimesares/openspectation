#!/usr/bin/env bash
# Builds the fixture workspace the demo recordings are made against.
#
# The workspace is regenerated from scratch on every run so a recording that
# toggles a task or edits a document never leaks into the next one. Modification
# times are set relative to now, which keeps the "updated" column and the
# dashboard's recent-activity list showing the same labels forever.
#
# Prose is written one paragraph per line, deliberately. The viewer wraps each
# source line on its own rather than reflowing a paragraph, so a document hard
# wrapped at eighty columns renders ragged (see src/tui/markdown.rs:21).

set -euo pipefail

ROOT="${1:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/workspace}"
PROJECT="$ROOT/atlas-api"
SPEC="$PROJECT/openspec"

rm -rf -- "$PROJECT"
mkdir -p "$SPEC"/{specs,changes/archive}

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

cat >"$SPEC/config.yaml" <<'YAML'
schema: spec-driven
context: |
  Atlas is the public API behind the Atlas dashboard: authentication, billing, webhooks and the sync engine the mobile clients talk to.

  Specifications are the source of truth. A capability is only considered shipped once its requirements carry scenarios and the change that introduced them has been archived.

  Conventions:

  - Requirements are written with RFC 2119 keywords and one scenario each.
  - A change proposal states why before it states what.
  - Breaking changes land behind a capability flag for one release.
project:
  name: Atlas API
  language: en
  owners:
    - platform-team
YAML

# ---------------------------------------------------------------------------
# Capabilities
# ---------------------------------------------------------------------------

spec() {
    local capability="$1"
    mkdir -p "$SPEC/specs/$capability"
    cat >"$SPEC/specs/$capability/spec.md"
}

spec auth <<'MD'
---
title: Authentication
status: approved
version: 2.4.0
owner: platform-team
tags: [core, security]
---

# auth Specification

## Purpose

Issuing, refreshing and revoking the credentials every other capability trusts.

## Requirements

### Requirement: Access tokens MUST expire

An access token SHALL be rejected once its `exp` claim is in the past, with no grace period.

#### Scenario: Expired access token

- **WHEN** a request carries a token whose `exp` is one second in the past
- **THEN** the API responds `401` with `code: token_expired`

### Requirement: Sessions MUST be revocable

Revoking a session SHALL invalidate every token minted for it, including tokens that have not yet expired.

#### Scenario: Revoked session rejects a live token

- **WHEN** a session is revoked while one of its access tokens is still valid
- **THEN** the next request using that token responds `401`

### Requirement: Password resets SHOULD be single use

A reset link SHALL stop working after the first successful use.

#### Scenario: Reset link replayed

- **WHEN** a reset link is opened a second time
- **THEN** the API responds `410` and no password is changed
MD

spec billing <<'MD'
---
title: Billing
status: implemented
version: 3.1.0
owner: revenue-team
tags: [core, money]
---

# billing Specification

## Purpose

Plans, invoices and the money that has to add up.

## Requirements

### Requirement: Invoices MUST balance

The sum of an invoice's line items SHALL equal its total, in minor units, with no rounding applied at render time.

#### Scenario: Line items disagree with the total

- **WHEN** an invoice is stored whose lines sum to a different total
- **THEN** the write is rejected before it reaches the ledger

### Requirement: Refunds MUST NOT exceed the charge

A refund SHALL be rejected when its amount is greater than the charge minus what has already been refunded.

#### Scenario: Second refund overshoots

- **WHEN** a charge of 4000 already refunded 3000 receives a refund of 1500
- **THEN** the API responds `422` with `code: refund_exceeds_charge`

### Requirement: Plan changes SHALL be prorated

#### Scenario: Upgrade mid-cycle

- **WHEN** a customer upgrades sixteen days into a thirty day cycle
- **THEN** the next invoice carries a credit for the unused remainder
MD

spec sync <<'MD'
---
title: Sync engine
status: draft
version: 0.4.0
owner: mobile-team
tags: [mobile, protocol]
---

# sync Specification

## Purpose

The delta protocol the mobile clients use to stay current.

## Requirements

### Requirement: Sync MUST be resumable

A client SHALL be able to resume from its last acknowledged cursor after any interruption, without replaying what it already applied.

#### Scenario: Connection drops mid-page

- **WHEN** a client reconnects with the cursor of the last applied page
- **THEN** the server resumes at the following record

### Requirement: Cursors MUST be opaque

#### Scenario: Client fabricates a cursor

- **WHEN** a request carries a cursor the server did not issue
- **THEN** the API responds `400` with `code: invalid_cursor`
MD

spec webhooks <<'MD'
---
title: Webhooks
status: review
version: 2.0.0
owner: platform-team
tags: [integrations]
---

# webhooks Specification

## Purpose

Outbound delivery, retries and the signature receivers verify.

## Requirements

### Requirement: Deliveries MUST be signed

Every request SHALL carry an `Atlas-Signature` header over the raw body and the timestamp, so a receiver can reject a replay.

#### Scenario: Body altered in transit

- **WHEN** a receiver recomputes the signature over a modified body
- **THEN** the signatures differ and the delivery is discarded

### Requirement: Failed deliveries SHALL retry with backoff

#### Scenario: Receiver returns 500 twice

- **WHEN** a receiver fails twice
- **THEN** the third attempt is scheduled at least four minutes later
MD

spec notifications <<'MD'
---
title: Notifications
status: approved
version: 1.2.0
owner: growth-team
tags: [email, push]
---

# notifications Specification

## Purpose

Email, push and the preferences that silence both.

## Requirements

### Requirement: Users MUST be able to opt out

An opt-out SHALL take effect before the next send, without a propagation window.

#### Scenario: Opt-out during a batch

- **WHEN** a user opts out while a batch naming them is in flight
- **THEN** that user is dropped from the batch
MD

# ---------------------------------------------------------------------------
# Active changes
# ---------------------------------------------------------------------------

mkdir -p "$SPEC/changes/add-oauth-refresh/specs/auth"

cat >"$SPEC/changes/add-oauth-refresh/proposal.md" <<'MD'
# Add silent OAuth refresh

## Why

Access tokens live fifteen minutes and the clients have no way to renew one without bouncing the user back through the consent screen. Support sees the same ticket every Monday morning: "it logged me out over the weekend".

Sixteen call sites currently catch `401` and redirect to `/login`. None of them distinguish an expired token from a revoked session, so a user whose session is perfectly valid is signed out for a recoverable error.

## What Changes

- A refresh token is issued alongside every access token, bound to the session rather than to the device.
- `POST /oauth/token` accepts `grant_type=refresh_token` and rotates the refresh token on every use, so a stolen one is single use.
- The `401` body distinguishes `token_expired` from `session_revoked`, which is what lets a client decide between refreshing and signing out.
- The sixteen call sites collapse into one interceptor.

## Impact

Clients keep working unchanged: the refresh flow is additive and the old behaviour is what happens when a client ignores it. The rotation rule is the breaking part for anyone who cached a refresh token, which is nobody today.
MD

cat >"$SPEC/changes/add-oauth-refresh/design.md" <<'MD'
# Design

## Rotation

A refresh token is single use. Presenting one mints a new pair and marks the presented token consumed; presenting a consumed token revokes the whole session, because the only way that happens is a token that leaked.

This costs a write on every refresh and makes a race between two clients of the same session fatal to both. That is the intended trade: a shared session is already the thing being protected against.

## Storage

Refresh tokens are stored hashed, next to the session, not in the token table. The token table is read on every request, and doubling its size to hold rows read once every fifteen minutes is the wrong shape.

## Rejected: sliding expiry

Extending the access token on each request removes the refresh flow entirely, but it also removes the revocation window: a token stays live as long as it is used, which is exactly what an attacker does with one.
MD

cat >"$SPEC/changes/add-oauth-refresh/tasks.md" <<'MD'
## 1. Protocol

- [x] 1.1 Write the delta for the `auth` capability
- [x] 1.2 Agree the error codes with the mobile clients
- [x] 1.3 Decide the rotation rule and write down what it costs

## 2. Server

- [x] 2.1 Issue a refresh token alongside every access token
- [x] 2.2 Accept `grant_type=refresh_token` on `POST /oauth/token`
- [x] 2.3 Rotate on use and consume the presented token
- [x] 2.4 Revoke the session when a consumed token is presented again
- [ ] 2.5 Split `401` into `token_expired` and `session_revoked`
- [ ] 2.6 Rate limit the refresh endpoint per session

## 3. Clients

- [x] 3.1 Collapse the sixteen `401` handlers into one interceptor
- [ ] 3.2 Queue in-flight requests while a refresh is running
- [ ] 3.3 Sign out only on `session_revoked`

## 4. Release

- [x] 4.1 Backfill refresh tokens for live sessions
- [ ] 4.2 Dashboard panel for refresh failures
- [ ] 4.3 Runbook entry for a leaked refresh token
MD

cat >"$SPEC/changes/add-oauth-refresh/specs/auth/spec.md" <<'MD'
## ADDED Requirements

### Requirement: Refresh tokens MUST rotate on use

Presenting a refresh token SHALL mint a new pair and consume the presented token. Presenting a consumed token SHALL revoke the session it belongs to.

#### Scenario: Refresh token replayed

- **WHEN** a refresh token that has already been used is presented again
- **THEN** the session is revoked and the API responds `401`

### Requirement: Refresh tokens MUST be bound to a session

A refresh token SHALL be rejected when presented for a different session, even by the same client.

#### Scenario: Token presented for another session

- **WHEN** a refresh token is presented with a mismatched session identifier
- **THEN** the API responds `401` with `code: invalid_grant`

## MODIFIED Requirements

### Requirement: Access tokens MUST expire

An access token SHALL be rejected once its `exp` claim is in the past. The rejection SHALL distinguish an expired token from a revoked session, so a client can tell a recoverable failure from a terminal one.

#### Scenario: Expired access token

- **WHEN** a request carries a token whose `exp` is one second in the past
- **THEN** the API responds `401` with `code: token_expired`

#### Scenario: Revoked session

- **WHEN** a request carries a valid token belonging to a revoked session
- **THEN** the API responds `401` with `code: session_revoked`

## REMOVED Requirements

### Requirement: Clients SHOULD re-authenticate on 401

Superseded by the refresh flow: a `401` is no longer a signal to send the user back through the consent screen.
MD

mkdir -p "$SPEC/changes/offline-first-sync/specs/sync"

cat >"$SPEC/changes/offline-first-sync/proposal.md" <<'MD'
# Offline-first sync

## Why

The mobile clients treat the network as a precondition. Underground, on a plane, or on hotel wifi the app is a spinner. Every write is lost on a disconnect because nothing is queued locally.

## What Changes

- Writes go to a local queue first and are drained when the connection returns.
- The delta protocol grows a conflict field so a client can be told its write lost, rather than silently discovering it later.
- Cursors survive a reinstall, so a returning device syncs a delta rather than the whole account.

## Impact

The wire format changes shape but not meaning: the conflict field is optional and absent for every write that did not conflict.
MD

cat >"$SPEC/changes/offline-first-sync/tasks.md" <<'MD'
## 1. Protocol

- [ ] 1.1 Write the delta for the `sync` capability
- [ ] 1.2 Decide what a conflict looks like on the wire
- [ ] 1.3 Version the cursor so an old client fails loudly

## 2. Client

- [ ] 2.1 Local write queue with a durable store
- [ ] 2.2 Drain on reconnect, oldest first
- [ ] 2.3 Surface a lost write instead of dropping it

## 3. Server

- [ ] 3.1 Detect a conflicting write and report it
- [ ] 3.2 Keep cursors valid across a reinstall
MD

cat >"$SPEC/changes/offline-first-sync/specs/sync/spec.md" <<'MD'
## ADDED Requirements

### Requirement: Writes MUST survive a disconnect

A write accepted by the client SHALL be persisted locally and delivered once connectivity returns, in the order it was made.

#### Scenario: Write made offline

- **WHEN** a write is made with no connection and the app is closed
- **THEN** the write is delivered on the next launch that reaches the network

### Requirement: Conflicts MUST be reported to the client

#### Scenario: Two devices write the same record

- **WHEN** a queued write targets a record changed since the client's cursor
- **THEN** the response carries the conflict and the winning revision
MD

mkdir -p "$SPEC/changes/retire-legacy-webhooks/specs/webhooks"

cat >"$SPEC/changes/retire-legacy-webhooks/proposal.md" <<'MD'
# Retire legacy webhooks

## Why

`Atlas-Signature-V1` is an HMAC over the body alone. Without a timestamp it cannot be replayed against, and every receiver that verifies it correctly is still vulnerable to a replay we cannot detect on our side either.

V2 has been sent alongside V1 for eleven months. Ninety-four percent of active endpoints verify V2.

## What Changes

- V1 stops being sent.
- The header name loses its version suffix, since there is one signature again.
- Receivers still on V1 get a deprecation delivery before the cutoff.

## Impact

Breaking for the six percent. They have been emailed twice and the dashboard has warned since March.
MD

cat >"$SPEC/changes/retire-legacy-webhooks/tasks.md" <<'MD'
## 1. Announce

- [x] 1.1 Dashboard warning on endpoints still verifying V1
- [x] 1.2 Second notice to the remaining endpoints
- [x] 1.3 Deprecation delivery scheduled

## 2. Remove

- [x] 2.1 Write the delta for the `webhooks` capability
- [ ] 2.2 Stop sending `Atlas-Signature-V1`
- [ ] 2.3 Rename the V2 header
- [ ] 2.4 Delete the V1 signing path
MD

cat >"$SPEC/changes/retire-legacy-webhooks/specs/webhooks/spec.md" <<'MD'
## RENAMED Requirements

### Requirement: Deliveries MUST be signed

Formerly "Deliveries MUST carry Atlas-Signature-V2". There is one signature again, so the requirement is named after what it does rather than after the version that survived.

#### Scenario: Body altered in transit

- **WHEN** a receiver recomputes the signature over a modified body
- **THEN** the signatures differ and the delivery is discarded

## REMOVED Requirements

### Requirement: Deliveries MUST carry Atlas-Signature-V1

An HMAC over the body with no timestamp cannot be replay-checked by either side, which is the whole reason V2 exists.
MD

# ---------------------------------------------------------------------------
# Archive
# ---------------------------------------------------------------------------

archived() {
    local id="$1"
    mkdir -p "$SPEC/changes/archive/$id"
    cat >"$SPEC/changes/archive/$id/proposal.md"
}

archived 2026-04-08-add-rate-limiting <<'MD'
# Add rate limiting

## Why

One customer's retry loop consumed forty percent of a shard for six hours. The only lever we had was blocking their key entirely.

## What Changes

- A token bucket per key, per route class.
- `429` carries `Retry-After` and the window that applies.

## Impact

Generous enough that no current customer would have been limited last month, except the retry loop.
MD

archived 2026-05-19-migrate-to-postgres-16 <<'MD'
# Migrate to PostgreSQL 16

## Why

Logical replication on 13 could not follow a schema change without a full resync, which made every migration a maintenance window.

## What Changes

- Rolling upgrade behind the pooler.
- Drop the two extensions that only existed to backport 16 behaviour.

## Impact

One window, six minutes, taken at the usual Sunday slot.
MD

archived 2026-06-30-unify-error-codes <<'MD'
# Unify error codes

## Why

Four spellings of "not found" across the API, three of them undocumented, and clients matched on the message string because the code was unreliable.

## What Changes

- One code per failure, listed in the specification for each capability.
- The message becomes prose that may change; the code becomes the contract.

## Impact

Clients matching on messages break, which is the point.
MD

cat >"$SPEC/changes/archive/2026-04-08-add-rate-limiting/tasks.md" <<'MD'
## 1. Build

- [x] 1.1 Token bucket per key and route class
- [x] 1.2 `Retry-After` on every `429`
- [x] 1.3 Dashboard panel per customer
MD

cat >"$SPEC/changes/archive/2026-05-19-migrate-to-postgres-16/tasks.md" <<'MD'
## 1. Prepare

- [x] 1.1 Replica on 16, following 13
- [x] 1.2 Drop the two backport extensions
- [x] 1.3 Rehearse the cutover on staging

## 2. Cut over

- [x] 2.1 Drain the pooler
- [x] 2.2 Promote and repoint
MD

cat >"$SPEC/changes/archive/2026-06-30-unify-error-codes/tasks.md" <<'MD'
## 1. Inventory

- [x] 1.1 Every code the API can emit, per capability
- [x] 1.2 Collapse the four spellings of "not found"

## 2. Land

- [x] 2.1 One code per failure, written into the specifications
- [x] 2.2 Deprecation notice for clients matching on messages
MD

# ---------------------------------------------------------------------------
# Modification times
#
# Relative to now so the "updated" column and the dashboard's recent-activity
# list read the same however long after seeding the recording is made.
# ---------------------------------------------------------------------------

stamp() { touch -d "$2" "$1"; }

stamp "$SPEC/changes/add-oauth-refresh/tasks.md" "35 minutes ago"
stamp "$SPEC/changes/add-oauth-refresh/proposal.md" "3 hours ago"
stamp "$SPEC/changes/add-oauth-refresh/design.md" "yesterday"
stamp "$SPEC/changes/add-oauth-refresh/specs/auth/spec.md" "5 hours ago"
stamp "$SPEC/changes/retire-legacy-webhooks/tasks.md" "2 days ago"
stamp "$SPEC/changes/retire-legacy-webhooks/proposal.md" "6 days ago"
stamp "$SPEC/changes/retire-legacy-webhooks/specs/webhooks/spec.md" "3 days ago"
stamp "$SPEC/changes/offline-first-sync/proposal.md" "9 days ago"
stamp "$SPEC/changes/offline-first-sync/tasks.md" "9 days ago"
stamp "$SPEC/changes/offline-first-sync/specs/sync/spec.md" "8 days ago"
stamp "$SPEC/specs/auth/spec.md" "4 hours ago"
stamp "$SPEC/specs/billing/spec.md" "11 days ago"
stamp "$SPEC/specs/sync/spec.md" "17 days ago"
stamp "$SPEC/specs/webhooks/spec.md" "23 days ago"
stamp "$SPEC/specs/notifications/spec.md" "2 months ago"
stamp "$SPEC/config.yaml" "3 months ago"
stamp "$SPEC/changes/archive/2026-04-08-add-rate-limiting/proposal.md" "4 months ago"
stamp "$SPEC/changes/archive/2026-04-08-add-rate-limiting/tasks.md" "4 months ago"
stamp "$SPEC/changes/archive/2026-05-19-migrate-to-postgres-16/proposal.md" "3 months ago"
stamp "$SPEC/changes/archive/2026-05-19-migrate-to-postgres-16/tasks.md" "3 months ago"
stamp "$SPEC/changes/archive/2026-06-30-unify-error-codes/proposal.md" "6 weeks ago"
stamp "$SPEC/changes/archive/2026-06-30-unify-error-codes/tasks.md" "6 weeks ago"

echo "seeded $PROJECT"

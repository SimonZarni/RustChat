//! TOTP multi-factor authentication through the real router, engine and PostgreSQL.
//! Requires `DATABASE_URL` (see README "Local services").

mod common;

use axum::http::{Method, StatusCode};
use common::{Options, PASSWORD, Reply, app, call, sign_in};
use rustchat_infrastructure::MIGRATOR;
use serde_json::{Value, json};
use sqlx::PgPool;

fn mfa_options() -> Options {
    Options {
        mfa: true,
        ..Options::default()
    }
}

/// The code an authenticator would show `steps_ahead` 30-second steps from now.
fn code_for(secret_base32: &str, steps_ahead: i64) -> String {
    let secret = totp_rs::Secret::try_from_base32(secret_base32).unwrap();
    let totp = totp_rs::Builder::new()
        .with_secret(secret.as_bytes().to_vec())
        .build()
        .unwrap();
    let now = time::OffsetDateTime::now_utc().unix_timestamp() + 30 * steps_ahead;
    totp.generate(u64::try_from(now).unwrap()).to_string()
}

struct Enrolled {
    secret: String,
    recovery_codes: Vec<String>,
}

/// Sign in, enrol TOTP and confirm it. The confirming code uses the current time step.
async fn enrol(app: &axum::Router, username: &str) -> (String, Enrolled) {
    let cookie = sign_in(app, username).await;
    let begun = call(
        app,
        Method::POST,
        "/api/v1/auth/mfa/totp",
        Some(json!({"current_password": PASSWORD})),
        Some(&cookie),
    )
    .await;
    assert_eq!(begun.status, StatusCode::OK, "{:?}", begun.json);
    let secret = begun.json["secret"].as_str().unwrap().to_owned();
    assert!(
        begun.json["otpauth_uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/RustChat:")
    );
    let confirmed = call(
        app,
        Method::POST,
        "/api/v1/auth/mfa/totp/confirm",
        Some(json!({"code": code_for(&secret, 0)})),
        Some(&cookie),
    )
    .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.json);
    let recovery_codes = confirmed.json["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    (
        cookie,
        Enrolled {
            secret,
            recovery_codes,
        },
    )
}

async fn password_login(app: &axum::Router, username: &str) -> Reply {
    call(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({"username": username, "password": PASSWORD})),
        None,
    )
    .await
}

async fn mfa_login(app: &axum::Router, token: &str, code: &str) -> Reply {
    call(
        app,
        Method::POST,
        "/api/v1/auth/login/mfa",
        Some(json!({"mfa_token": token, "code": code})),
        None,
    )
    .await
}

fn challenge(reply: &Reply) -> String {
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{:?}", reply.json);
    assert_eq!(reply.json["mfa_required"], true);
    reply.json["mfa_token"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn mfa_is_reported_unavailable_without_a_server_key(pool: PgPool) {
    let app = app(pool, Options::default());
    let cookie = sign_in(&app, "alice").await;

    let status = call(&app, Method::GET, "/api/v1/auth/mfa", None, Some(&cookie)).await;
    assert_eq!(
        status.json["mfa"],
        json!({"available": false, "enabled": false, "recovery_codes_remaining": 0})
    );
    let begin = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp",
        Some(json!({"current_password": PASSWORD})),
        Some(&cookie),
    )
    .await;
    assert_eq!(begin.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(begin.code(), "MFA_UNAVAILABLE");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn enrolment_needs_the_password_and_a_working_authenticator(pool: PgPool) {
    let app = app(pool.clone(), mfa_options());
    let cookie = sign_in(&app, "alice").await;
    let state = |reply: &Reply| reply.json["mfa"].clone();

    let before = call(&app, Method::GET, "/api/v1/auth/mfa", None, Some(&cookie)).await;
    assert_eq!(
        state(&before),
        json!({"available": true, "enabled": false, "recovery_codes_remaining": 0})
    );

    let wrong = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp",
        Some(json!({"current_password": "wrong password!!"})),
        Some(&cookie),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    let anonymous = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp",
        Some(json!({"current_password": PASSWORD})),
        None,
    )
    .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let begun = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp",
        Some(json!({"current_password": PASSWORD})),
        Some(&cookie),
    )
    .await;
    let secret = begun.json["secret"].as_str().unwrap();
    // Pending: no effect on login, and a wrong code does not confirm.
    assert_eq!(password_login(&app, "alice").await.status, StatusCode::OK);
    let bad = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp/confirm",
        Some(json!({"code": "000000"})),
        Some(&cookie),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad.code(), "INVALID_MFA_CODE");

    let confirmed = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp/confirm",
        Some(json!({"code": code_for(secret, 0)})),
        Some(&cookie),
    )
    .await;
    assert_eq!(confirmed.status, StatusCode::OK);
    assert_eq!(
        confirmed.json["recovery_codes"].as_array().unwrap().len(),
        10
    );
    let after = call(&app, Method::GET, "/api/v1/auth/mfa", None, Some(&cookie)).await;
    assert_eq!(
        state(&after),
        json!({"available": true, "enabled": true, "recovery_codes_remaining": 10})
    );

    // Enrolling again while enabled is refused.
    let again = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/totp",
        Some(json!({"current_password": PASSWORD})),
        Some(&cookie),
    )
    .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "MFA_ALREADY_ENABLED");

    // At rest: the secret is encrypted and recovery codes are only digests.
    let stored: (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT secret_ciphertext, secret_nonce FROM mfa_totp")
            .fetch_one(&pool)
            .await
            .unwrap();
    let plain = totp_rs::Secret::try_from_base32(secret).unwrap();
    assert!(
        !stored
            .0
            .windows(plain.as_bytes().len())
            .any(|w| w == plain.as_bytes())
    );
    assert_eq!(stored.1.len(), 12);
    let digests: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mfa_recovery_codes WHERE octet_length(code_digest) = 32",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(digests, 10);
    for code in confirmed.json["recovery_codes"].as_array().unwrap() {
        let plain_code = code.as_str().unwrap().replace('-', "");
        let leaked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM mfa_recovery_codes WHERE code_digest = convert_to($1, 'UTF8')",
        )
        .bind(&plain_code)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(leaked, 0);
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn login_needs_a_second_factor_and_each_code_works_once(pool: PgPool) {
    let app = app(pool, mfa_options());
    let (_, enrolled) = enrol(&app, "alice").await;

    // The password alone yields a challenge, not a session.
    let first = password_login(&app, "alice").await;
    assert!(
        first.session_cookies().is_empty(),
        "no cookie before the second factor"
    );
    let token = challenge(&first);
    let me = call(&app, Method::GET, "/api/v1/auth/me", None, None).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);

    let wrong = mfa_login(&app, &token, "000000").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "INVALID_MFA_CODE");
    assert!(wrong.session_cookies().is_empty());

    // The confirming code is spent; the next step's code (within the allowed skew) signs in.
    let stale = mfa_login(&app, &token, &code_for(&enrolled.secret, 0)).await;
    assert_eq!(
        stale.status,
        StatusCode::UNAUTHORIZED,
        "the enrolment code cannot be replayed"
    );
    let next = code_for(&enrolled.secret, 1);
    let signed_in = mfa_login(&app, &token, &next).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{:?}", signed_in.json);
    assert_eq!(signed_in.json["user"]["username"], "alice");
    let cookie = signed_in
        .new_session_cookie()
        .expect("session after both factors");
    let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::OK);

    // The same code on a fresh challenge, and the used challenge itself, both fail.
    let second = challenge(&password_login(&app, "alice").await);
    assert_eq!(
        mfa_login(&app, &second, &next).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mfa_login(&app, &token, &next).await.status,
        StatusCode::UNAUTHORIZED
    );

    // A recovery code signs in once, in any case and with or without separators.
    let recovery = enrolled.recovery_codes[0].to_uppercase().replace('-', " ");
    let by_recovery = mfa_login(&app, &second, &recovery).await;
    assert_eq!(by_recovery.status, StatusCode::OK, "{:?}", by_recovery.json);
    let third = challenge(&password_login(&app, "alice").await);
    assert_eq!(
        mfa_login(&app, &third, &enrolled.recovery_codes[0])
            .await
            .status,
        StatusCode::UNAUTHORIZED,
        "a recovery code works only once"
    );
    assert_eq!(
        mfa_login(&app, &third, &enrolled.recovery_codes[1])
            .await
            .status,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_challenge_allows_only_a_few_guesses(pool: PgPool) {
    let app = app(pool, mfa_options());
    let (_, enrolled) = enrol(&app, "alice").await;
    let token = challenge(&password_login(&app, "alice").await);

    for _ in 0..5 {
        assert_eq!(
            mfa_login(&app, &token, "111111").await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    // Even a correct code is refused once the attempts are spent; sign in again to retry.
    assert_eq!(
        mfa_login(&app, &token, &code_for(&enrolled.secret, 1))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let fresh = challenge(&password_login(&app, "alice").await);
    assert_eq!(
        mfa_login(&app, &fresh, &code_for(&enrolled.secret, 1))
            .await
            .status,
        StatusCode::OK
    );
    // Garbage and oversized challenges fail the same way.
    assert_eq!(
        mfa_login(&app, "nope", "123456").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mfa_login(&app, &"x".repeat(500), "123456").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn disabling_and_regenerating_need_the_password_and_a_code(pool: PgPool) {
    let app = app(pool, mfa_options());
    let (cookie, enrolled) = enrol(&app, "alice").await;
    let code = code_for(&enrolled.secret, 1);
    let post = |path: &'static str, body: Value| {
        let (app, cookie) = (app.clone(), cookie.clone());
        async move { call(&app, Method::POST, path, Some(body), Some(&cookie)).await }
    };

    let wrong_password = post(
        "/api/v1/auth/mfa/disable",
        json!({"current_password": "wrong password!!", "code": code}),
    )
    .await;
    assert_eq!(wrong_password.status, StatusCode::UNAUTHORIZED);
    let wrong_code = post(
        "/api/v1/auth/mfa/disable",
        json!({"current_password": PASSWORD, "code": "000000"}),
    )
    .await;
    assert_eq!(wrong_code.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_code.code(), "INVALID_MFA_CODE");

    // Regenerate with a recovery code as the second factor; the old codes stop working.
    let regenerated = post(
        "/api/v1/auth/mfa/recovery-codes",
        json!({"current_password": PASSWORD, "code": enrolled.recovery_codes[0]}),
    )
    .await;
    assert_eq!(regenerated.status, StatusCode::OK, "{:?}", regenerated.json);
    let fresh: Vec<String> = regenerated.json["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(fresh.len(), 10);
    let token = challenge(&password_login(&app, "alice").await);
    assert_eq!(
        mfa_login(&app, &token, &enrolled.recovery_codes[1])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // Disable with the right password and a valid code; login is a plain password login again.
    let disabled = post(
        "/api/v1/auth/mfa/disable",
        json!({"current_password": PASSWORD, "code": fresh[0]}),
    )
    .await;
    assert_eq!(
        disabled.status,
        StatusCode::NO_CONTENT,
        "{:?}",
        disabled.json
    );
    let status = call(&app, Method::GET, "/api/v1/auth/mfa", None, Some(&cookie)).await;
    assert_eq!(status.json["mfa"]["enabled"], false);
    assert_eq!(password_login(&app, "alice").await.status, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_stolen_session_cannot_turn_the_second_factor_off(pool: PgPool) {
    let app = app(pool, mfa_options());
    let (cookie, _) = enrol(&app, "alice").await;

    // Knowing the password and holding a session is not enough: a code is required.
    let attempt = call(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/disable",
        Some(json!({"current_password": PASSWORD, "code": "123456"})),
        Some(&cookie),
    )
    .await;
    assert_eq!(attempt.status, StatusCode::UNAUTHORIZED);
    let status = call(&app, Method::GET, "/api/v1/auth/mfa", None, Some(&cookie)).await;
    assert_eq!(status.json["mfa"]["enabled"], true);
}

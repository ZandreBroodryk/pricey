//! Transactional email via Resend.
//!
//! Only one message is sent by this application (address verification), so this is a
//! deliberately small wrapper rather than a general mailer.

use serde_json::json;

/// Where verification links point. Must be the address the browser will actually reach,
/// so it differs between local development and the deployment.
fn base_url() -> String {
    std::env::var("APP_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string())
}

pub fn verification_link(token: &str) -> String {
    format!("{}/verify?token={token}", base_url().trim_end_matches('/'))
}

/// Sends the verification email.
///
/// With no `RESEND_API_KEY` configured the link is logged instead of sent. That keeps
/// local development and the test suite working without an account or network access --
/// and it is logged at WARN precisely so it is obvious this is not production behaviour.
pub async fn send_verification(to: &str, link: &str) -> Result<(), String> {
    let Ok(api_key) = std::env::var("RESEND_API_KEY") else {
        tracing::warn!(
            recipient = %to,
            %link,
            "RESEND_API_KEY is not set; logging the verification link instead of sending it"
        );
        return Ok(());
    };

    let from = std::env::var("EMAIL_FROM")
        .unwrap_or_else(|_| "pricey <onboarding@resend.dev>".to_string());

    let response = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .bearer_auth(api_key)
        .json(&json!({
            "from": from,
            "to": [to],
            "subject": "Confirm your pricey account",
            "text": text_body(link),
            "html": html_body(link),
        }))
        .send()
        .await
        .map_err(|e| format!("could not reach Resend: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        // Resend puts the reason in the body; without it the log says nothing useful.
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Resend rejected the message ({status}): {body}"));
    }

    Ok(())
}

fn text_body(link: &str) -> String {
    format!(
        "Confirm your email address to start using pricey:\n\n{link}\n\n\
         This link expires in 24 hours. If you did not create this account, ignore this email."
    )
}

fn html_body(link: &str) -> String {
    // Deliberately plain: inline styles and a table-free layout survive email clients,
    // and there is nothing here worth the fragility of a richer template.
    format!(
        r#"<!doctype html>
<html><body style="font-family:system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;line-height:1.5;color:#17171a">
  <h1 style="font-size:20px;margin:0 0 12px">Confirm your email</h1>
  <p style="margin:0 0 20px">Confirm your address to start using pricey.</p>
  <p style="margin:0 0 20px">
    <a href="{link}" style="display:inline-block;background:#2f6fed;color:#fff;text-decoration:none;padding:10px 18px;border-radius:8px">Confirm email address</a>
  </p>
  <p style="margin:0 0 8px;color:#6b6b76;font-size:13px">Or paste this link into your browser:</p>
  <p style="margin:0 0 20px;word-break:break-all;font-size:13px"><a href="{link}">{link}</a></p>
  <p style="margin:0;color:#6b6b76;font-size:13px">This link expires in 24 hours. If you did not create this account, you can ignore this email.</p>
</body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_link_against_the_configured_base_url() {
        // Not asserting on APP_BASE_URL itself: env vars are process-global and would make
        // this test order-dependent. The join behaviour is what matters.
        let link = verification_link("abc-123");
        assert!(link.ends_with("/verify?token=abc-123"), "got {link}");
        assert!(
            !link.contains("//verify"),
            "base url slash must not double up"
        );
    }

    #[test]
    fn both_bodies_carry_the_link() {
        let link = "https://example.com/verify?token=tok";
        assert!(text_body(link).contains(link));
        assert!(html_body(link).contains(link));
    }
}

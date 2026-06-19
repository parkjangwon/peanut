use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};

use crate::config::MailConfig;

pub async fn send_email(
    config: &MailConfig,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    if !config.smtp_enabled {
        tracing::info!(
            to = to,
            subject = subject,
            body = body,
            "SMTP not configured; logging email instead of sending"
        );
        return Ok(());
    }

    let message = Message::builder()
        .from(
            config
                .smtp_from
                .parse()
                .map_err(|_| "SMTP_FROM must be a valid email address".to_string())?,
        )
        .to(to
            .parse()
            .map_err(|_| "recipient must be a valid email address".to_string())?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|error| format!("failed to build email: {error}"))?;

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
        .map_err(|error| format!("failed to configure SMTP relay: {error}"))?
        .port(config.smtp_port)
        .credentials(Credentials::new(
            config.smtp_user.clone(),
            config.smtp_password.clone(),
        ))
        .build();

    mailer
        .send(message)
        .await
        .map_err(|error| format!("failed to send email: {error}"))?;

    Ok(())
}

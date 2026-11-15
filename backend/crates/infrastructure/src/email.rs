//! SMTP delivery for account verification and password recovery.

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use rustchat_application::{error::AppError, ports::EmailDelivery};
use rustchat_domain::identity::EmailAddress;

pub struct SmtpEmailDelivery {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: lettre::message::Mailbox,
}

impl SmtpEmailDelivery {
    pub fn new(
        host: &str,
        port: u16,
        username: Option<&str>,
        password: Option<&str>,
        from: &str,
    ) -> Result<Self, AppError> {
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
            .map_err(AppError::internal)?
            .port(port);
        if let (Some(username), Some(password)) = (username, password) {
            builder =
                builder.credentials(Credentials::new(username.to_owned(), password.to_owned()));
        }
        let from = from.parse().map_err(AppError::internal)?;
        Ok(Self {
            transport: builder.build(),
            from,
        })
    }
}

#[async_trait::async_trait]
impl EmailDelivery for SmtpEmailDelivery {
    async fn send(
        &self,
        recipient: &EmailAddress,
        subject: &str,
        text: &str,
    ) -> Result<(), AppError> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(recipient.as_str().parse().map_err(AppError::internal)?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(text.to_owned())
            .map_err(AppError::internal)?;
        self.transport
            .send(message)
            .await
            .map_err(AppError::internal)?;
        Ok(())
    }
}

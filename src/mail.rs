use chrono::{DateTime, Utc};
use mail_send::{mail_builder::MessageBuilder, SmtpClientBuilder};
use std::env::var;

pub async fn task_done(file_name: &str, time: &DateTime<Utc>) {
    let (addr, pass) = match get_vars() {
        Some(x) => x,
        None => return,
    };
    let addr = addr.as_str();

    let html_body = format!(
        r#"
<p>Tisztelt Tanár úr!</p>

<p>
Sikeresen lement a következő adás: <br/>
Név: <b>{}</b> <br/>
Időpont: <b>{}</b>
</p>

<p>Varga Benedek</p>"#,
        file_name,
        time.naive_local()
    );

    let text_body = format!(
        r#"
Tisztelt Tanár úr!

Sikeresen lement a következő adás:
Név: {}
Időpont: {}

Varga Benedek"#,
        file_name,
        time.naive_local()
    );

    let message = MessageBuilder::new()
        .from(("Csengő Mail", addr))
        .to(addr)
        .reply_to(addr)
        .subject(format!("Adás: {}", file_name))
        .html_body(html_body.trim())
        .text_body(text_body.trim());

    let mut transport = match SmtpClientBuilder::new("smtp.gmail.com", 465)
        .implicit_tls(true)
        .credentials((addr, pass.as_str()))
        .connect()
        .await
    {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to connect to smtp server\n{:#?}", e);
            return;
        }
    };
    match transport.send(message).await {
        Ok(_) => {
            info!("Mail sent");
        }
        Err(e) => {
            error!("Failed to send mail\n{:#?}", e);
        }
    }
}

pub fn get_vars() -> Option<(String, String)> {
    let addr = var("MAIL_ADDR");
    let pass = var("MAIL_PASS");
    if addr.is_err() || pass.is_err() {
        warn!("MAIL_ADDR or MAIL_PASS not set, no mail will be sent");
        return None;
    }
    Some((addr.unwrap(), pass.unwrap()))
}

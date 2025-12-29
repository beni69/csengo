use chrono::{DateTime, Local};
use mail_send::{mail_builder::MessageBuilder, SmtpClientBuilder};
use std::env::var;

const DEFAULT_SIGNATURE: &str = "Stúdiósok";

pub async fn task_done(file_name: &str, time: &DateTime<Local>) {
    let (addr, pass, signature) = match get_vars() {
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

 <p>Üdvözlettel,<br/>{signature}</p>"#,
        file_name,
        time.naive_local()
    );

    let text_body = format!(
        r#"
Tisztelt Tanár úr!

Sikeresen lement a következő adás:
Név: {}
Időpont: {}

Üdvözlettel,
{signature}"#,
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

pub fn get_vars() -> Option<(String, String, String)> {
    match (var("MAIL_ADDR"), var("MAIL_PASS"), var("MAIL_SIGNATURE")) {
        (Ok(addr), Ok(pass), Ok(signature)) => Some((addr, pass, signature)),
        (Ok(addr), Ok(pass), Err(_)) => {
            warn!("MAIL_SIGNATURE not set, using default");
            Some((addr, pass, DEFAULT_SIGNATURE.to_string()))
        }
        _ => {
            warn!("MAIL_ADDR or MAIL_PASS not set, no mail will be sent");
            None
        }
    }
}

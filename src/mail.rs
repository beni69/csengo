use chrono::{DateTime, Utc};
use mail_send::{mail_builder::MessageBuilder, Transport};
use std::env::var;

pub(crate) async fn task_done(file_name: &str, time: &DateTime<Utc>) {
    let (addr, pass) = match get_vars() {
        Some(x) => x,
        None => return,
    };
    let addr = addr.as_str();

    let body = format!(
        r#"
Tisztelt Tanár úr! <br/>
<br/>
Sikeresen lement a következő adás: <br/>
Név: <b>{}</b> <br/>
Időpont: <b>{}</b> <br/>
<br/>
Varga Benedek
        "#,
        file_name,
        time.naive_local()
    );

    let message = MessageBuilder::new()
        .from(("Csengő Mail", addr))
        .to(addr)
        .reply_to(addr)
        .subject(format!("Adás: {}", file_name))
        .html_body(body.trim());

    let mut transport = match Transport::new("smtp.gmail.com")
        .credentials(addr, pass.as_str())
        .connect_tls()
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

pub(crate) fn get_vars() -> Option<(String, String)> {
    let addr = var("MAIL_ADDR");
    let pass = var("MAIL_PASS");
    if addr.is_err() || pass.is_err() {
        warn!("MAIL_ADDR or MAIL_PASS not set, no mail will be sent");
        return None;
    }
    Some((addr.unwrap(), pass.unwrap()))
}

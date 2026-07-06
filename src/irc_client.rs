use futures::prelude::*;
use irc::client::prelude::*;
use tokio::sync::mpsc;

use crate::config::ServerConfig;

#[derive(Debug, Clone)]
pub struct IrcMessage {
    pub server: usize,
    pub target: String,
    pub sender: Option<String>,
    pub text: String,
    pub kind: MessageKind,
    pub time: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageKind {
    Chat,
    Action,
    Notice,
    Join,
    Part,
    Quit,
    Topic,
    Server,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum IrcCommand {
    Join(usize, String),
    Part(usize, String),
    SendMsg(usize, String, String),
    SendAction(usize, String, String),
    Disconnect,
}

#[derive(Debug, Clone)]
pub struct ServerState {
    pub idx: usize,
    pub name: String,
    pub connection: ConnectionState,
    pub channels: Vec<String>,
    pub current_nick: String,
}

pub fn spawn_connection(
    idx: usize,
    cfg: ServerConfig,
) -> (
    mpsc::UnboundedSender<IrcCommand>,
    mpsc::UnboundedReceiver<IrcMessage>,
) {
    let (msg_tx, msg_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(e) = run_connection(idx, cfg, &msg_tx, cmd_rx).await {
            let _ = msg_tx.send(server_msg(idx, &format!("Error: {e}")));
        }
    });

    (cmd_tx, msg_rx)
}

async fn run_connection(
    idx: usize,
    cfg: ServerConfig,
    msg_tx: &mpsc::UnboundedSender<IrcMessage>,
    mut cmd_rx: mpsc::UnboundedReceiver<IrcCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let irc_config = Config {
        nickname: Some(cfg.nick.clone()),
        username: cfg.user.clone(),
        realname: cfg.realname.clone(),
        server: Some(cfg.host.clone()),
        port: Some(cfg.port),
        use_tls: Some(cfg.use_tls),
        password: cfg.password.clone(),
        channels: cfg.channels.clone(),
        ..Default::default()
    };

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;

    let nick = client.current_nickname().to_string();
    let _ = msg_tx.send(server_msg(idx, &format!("Connected as {nick}")));

    let sender = client.sender();
    let mut msg_stream = client.stream()?;

    let cmd_msg_tx = msg_tx.clone();
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                IrcCommand::Join(_, ch) => {
                    let _ = sender.send_join(&ch);
                }
                IrcCommand::Part(_, ch) => {
                    let _ = sender.send_part(&ch);
                }
                IrcCommand::SendMsg(_, target, text) => {
                    let _ = sender.send_privmsg(&target, &text);
                }
                IrcCommand::SendAction(_, target, text) => {
                    let _ = sender.send_action(&target, &text);
                }
                IrcCommand::Disconnect => {
                    let _ = sender.send_quit("COSMIC Chat signed off");
                    let _ = cmd_msg_tx.send(server_msg(idx, "Disconnected"));
                    return;
                }
            }
        }
    });

    while let Some(message) = msg_stream.next().await {
        match message {
            Ok(msg) => {
                if let Some(irc_msg) = parse_message(idx, &msg) {
                    let _ = msg_tx.send(irc_msg);
                }
            }
            Err(_) => {
                let _ = msg_tx.send(server_msg(idx, "Connection error"));
                break;
            }
        }
    }

    let _ = msg_tx.send(server_msg(idx, "Disconnected"));
    Ok(())
}

fn parse_message(idx: usize, msg: &Message) -> Option<IrcMessage> {
    let (kind, sender, text, target) = match &msg.command {
        Command::PRIVMSG(target, text) => {
            if text.starts_with("\u{1}ACTION ") && text.ends_with('\u{1}') {
                let body = &text[8..text.len() - 1];
                (
                    MessageKind::Action,
                    msg.source_nickname().map(|s| s.to_string()),
                    body.to_string(),
                    target.clone(),
                )
            } else {
                (
                    MessageKind::Chat,
                    msg.source_nickname().map(|s| s.to_string()),
                    text.clone(),
                    target.clone(),
                )
            }
        }
        Command::NOTICE(target, text) => (
            MessageKind::Notice,
            msg.source_nickname().map(|s| s.to_string()),
            format!("[{target}] {text}"),
            String::new(),
        ),
        Command::JOIN(ch, _, _) => {
            let nick = msg.source_nickname().unwrap_or("?").to_string();
            (
                MessageKind::Join,
                Some(nick.clone()),
                format!("{nick} joined"),
                ch.clone(),
            )
        }
        Command::PART(ch, reason) => {
            let nick = msg.source_nickname().unwrap_or("?").to_string();
            let extra = reason.as_deref().unwrap_or("");
            (
                MessageKind::Part,
                Some(nick.clone()),
                format!("{nick} left ({extra})"),
                ch.clone(),
            )
        }
        Command::QUIT(reason) => {
            let nick = msg.source_nickname().unwrap_or("?").to_string();
            let extra = reason.as_deref().unwrap_or("");
            (
                MessageKind::Quit,
                Some(nick.clone()),
                format!("{nick} quit ({extra})"),
                String::new(),
            )
        }
        Command::TOPIC(ch, topic) => {
            let nick = msg.source_nickname().map(|s| s.to_string());
            let t = topic.as_deref().unwrap_or("");
            (
                MessageKind::Topic,
                nick.clone(),
                match nick {
                    Some(n) => format!("{n} set topic: {t}"),
                    None => format!("Topic: {t}"),
                },
                ch.clone(),
            )
        }
        _ => return None,
    };

    Some(IrcMessage {
        server: idx,
        target,
        sender,
        text,
        kind,
        time: now_hhmmss(),
    })
}

fn server_msg(idx: usize, text: &str) -> IrcMessage {
    IrcMessage {
        server: idx,
        target: String::new(),
        sender: None,
        text: text.to_string(),
        kind: MessageKind::Server,
        time: now_hhmmss(),
    }
}

fn now_hhmmss() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = t.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

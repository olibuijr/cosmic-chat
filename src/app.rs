use std::collections::HashMap;

use cosmic::app::{Core, Task, context_drawer};
use cosmic::iced::{Alignment, Length, Subscription};
use cosmic::widget::{self, nav_bar, space};
use cosmic::Element;
use tokio::sync::mpsc;

use crate::config::{Config, ServerConfig, UserProfile};
use crate::irc_client::{
    ConnectionConfig, ConnectionState, IrcCommand, IrcMessage, MessageKind,
    ServerState, spawn_connection,
};

// ── Messages ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    NavSelect(usize),
    InputChanged(String),
    SendMessage,
    ToggleContextPage(ContextPage),
    Connect(usize),
    Disconnect(usize),
    Tick,
    AddServerDialog,
    AddServer(ServerConfig),
    Noop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContextPage {
    ServerInfo(usize),
    About,
}

#[derive(Debug, Clone, PartialEq)]
enum NavItem {
    Server(usize),
    Channel(usize, String),
}

// ── Application model ───────────────────────────────────────────────────────

pub struct CosmicChat {
    core: Core,
    nav: nav_bar::Model,
    config: Config,
    servers: Vec<ServerState>,
    cmd_txs: HashMap<usize, mpsc::UnboundedSender<IrcCommand>>,
    msg_rx: Option<mpsc::UnboundedReceiver<IrcMessage>>,
    messages: Vec<IrcMessage>,
    selected: Option<NavItem>,
    input: String,
    context_page: ContextPage,
    dialog_server: ServerConfig,
    dialog_channels_str: String,
    dialog_profile: String,
    dialog_show: bool,
    /// Last message index we notified for (avoids re-notifying).
    last_notified: usize,
    /// Per-channel user lists: (server_idx, channel) -> sorted nicknames
    channel_users: HashMap<(usize, String), Vec<String>>,
    /// Per-channel topics: (server_idx, channel) -> topic text
    channel_topics: HashMap<(usize, String), String>,
}

impl cosmic::Application for CosmicChat {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "com.cosmic.Chat";

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let config = Config::ensure_default();

        let servers: Vec<ServerState> = config
            .servers
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let profile = config.resolve_profile(s);
                ServerState {
                    idx: i,
                    name: s.name.clone(),
                    connection: ConnectionState::Disconnected,
                    channels: s.channels.clone(),
                    current_nick: profile.nick.clone(),
                }
            })
            .collect();

        let mut nav = nav_bar::Model::default();
        rebuild_nav(&mut nav, &servers);

        let app = CosmicChat {
            core,
            nav,
            config,
            servers,
            cmd_txs: HashMap::new(),
            msg_rx: None,
            messages: Vec::new(),
            selected: None,
            input: String::new(),
            context_page: ContextPage::About,
            dialog_server: ServerConfig::default(),
            dialog_channels_str: String::new(),
            dialog_profile: "default".into(),
            dialog_show: false,
            channel_users: HashMap::new(),
            channel_topics: HashMap::new(),
            last_notified: 0,
        };

        (app, Task::none())
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Message> {
        if let Some(item) = self.nav.data::<NavItem>(id) {
            self.selected = Some(item.clone());
        }
        self.nav.activate(id);
        Task::none()
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        let sp = cosmic::theme::spacing();
        let mut row = widget::row::with_capacity(3).spacing(sp.space_xs);

        let (status_icon, status_text) = match &self.selected {
            Some(NavItem::Server(i)) | Some(NavItem::Channel(i, _)) => {
                match self.servers.get(*i).map(|s| &s.connection) {
                    Some(ConnectionState::Connected) => ("emblem-ok-symbolic", "Connected"),
                    Some(ConnectionState::Connecting) => ("emblem-synchronizing-symbolic", "Connecting..."),
                    Some(ConnectionState::Error(_)) => ("dialog-error-symbolic", "Error"),
                    _ => ("network-offline-symbolic", "Disconnected"),
                }
            }
            _ => ("network-offline-symbolic", "Disconnected"),
        };

        row = row
            .push(widget::icon::from_name(status_icon).size(16))
            .push(widget::text::body(status_text));

        if let Some(NavItem::Server(i)) | Some(NavItem::Channel(i, _)) = &self.selected {
            if let Some(s) = self.servers.get(*i) {
                row = row.push(widget::text::caption(format!("  {}", s.name)));
            }
        }

        vec![row.into()]
    }

    fn header_end(&self) -> Vec<Element<'_, Message>> {
        let mut items: Vec<Element<'_, Message>> = Vec::new();

        if let Some(NavItem::Server(i)) | Some(NavItem::Channel(i, _)) = &self.selected {
            let is_connected = self.servers
                .get(*i)
                .map(|s| s.connection == ConnectionState::Connected)
                .unwrap_or(false);

            if is_connected {
                items.push(
                    widget::button::standard("Disconnect")
                        .on_press(Message::Disconnect(*i))
                        .into(),
                );
            } else {
                items.push(
                    widget::button::suggested("Connect")
                        .on_press(Message::Connect(*i))
                        .into(),
                );
            }
        }

        items.push(
            widget::button::standard("+ Server")
                .on_press(Message::AddServerDialog)
                .into(),
        );

        items.push(
            widget::button::standard("Info")
                .on_press(Message::ToggleContextPage(ContextPage::About))
                .into(),
        );

        items
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Message>> {
        if !self.core.window.show_context {
            return None;
        }

        match &self.context_page {
            ContextPage::ServerInfo(i) => {
                let body = if let Some(s) = self.servers.get(*i) {
                    let sc = self.config.servers.get(*i);
                    let host = sc.map(|c| c.host.as_str()).unwrap_or("?");
                    let port = sc.map(|c| c.port).unwrap_or(0);
                    let profile = sc.map(|c| c.profile.as_str()).unwrap_or("default");
                    widget::text::body(format!(
                        "Server: {}\nHost: {}:{}\nNick: {}\nProfile: {}\nStatus: {:?}\nChannels: {}",
                        s.name, host, port, s.current_nick, profile, s.connection,
                        s.channels.join(", "),
                    ))
                } else {
                    widget::text::body("No server selected")
                };

                Some(context_drawer::context_drawer(
                    Element::from(body),
                    Message::ToggleContextPage(ContextPage::ServerInfo(*i)),
                ))
            }
            ContextPage::About => {
                let body = widget::text::body(format!(
                    "COSMIC Chat v{}\n\nA native IRC client for the COSMIC desktop.\nBuilt with libcosmic + Rust.\n\nServers: {}\nProfiles: {}",
                    env!("CARGO_PKG_VERSION"),
                    self.config.servers.len(),
                    self.config.profile.names().len(),
                ));
                Some(
                    context_drawer::context_drawer(
                        Element::from(body),
                        Message::ToggleContextPage(ContextPage::About),
                    )
                    .title("About"),
                )
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        if self.dialog_show {
            return self.view_add_server_dialog();
        }

        match &self.selected {
            Some(NavItem::Server(i)) => self.view_server(*i),
            Some(NavItem::Channel(i, ch)) => self.view_channel(*i, ch),
            None => self.view_welcome(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            cosmic::iced::time::every(std::time::Duration::from_millis(100))
                .map(|_| Message::Tick),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                // Drain IRC messages
                if let Some(ref mut rx) = self.msg_rx {
                    while let Ok(msg) = rx.try_recv() {
                        let key = (msg.server, msg.target.clone());
                        match msg.kind {
                            MessageKind::Join => {
                                // Track user
                                if let Some(ref sender) = msg.sender {
                                    self.channel_users
                                        .entry(key.clone())
                                        .or_default()
                                        .push(sender.clone());
                                }
                                // Track channel
                                if let Some(s) = self.servers.get_mut(msg.server) {
                                    if !s.channels.contains(&msg.target) {
                                        s.channels.push(msg.target.clone());
                                        s.channels.sort();
                                    }
                                }
                            }
                            MessageKind::Part | MessageKind::Quit => {
                                if let Some(ref sender) = msg.sender {
                                    if let Some(users) = self.channel_users.get_mut(&key) {
                                        users.retain(|u| u != sender);
                                    }
                                }
                            }
                            MessageKind::Topic => {
                                self.channel_topics.insert(key, msg.text.clone());
                            }
                            _ => {}
                        }
                        self.messages.push(msg);
                    }
                }

                // ── Notifications + sound ──────────────────────────
                let layout = &self.config.layout;
                let new_msgs = &self.messages[self.last_notified..];
                for msg in new_msgs {
                    // Only notify for incoming chat/action messages from others
                    if matches!(msg.kind, MessageKind::Chat | MessageKind::Action) {
                        if let Some(ref nick) = msg.sender {
                            // Don't notify for our own messages
                            let is_self = self.servers.get(msg.server)
                                .map(|s| s.current_nick == *nick)
                                .unwrap_or(false);
                            if is_self { continue; }

                            let channel = &msg.target;
                            let body = {
                                let s = nick;
                                let t = &msg.text;
                                if msg.kind == MessageKind::Action {
                                    format!("* {s} {t}")
                                } else {
                                    format!("<{s}> {t}")
                                }
                            };

                            if layout.notifications {
                                let _ = std::process::Command::new("notify-send")
                                    .args(["--app-name", "COSMIC Chat"])
                                    .args(["--category", "im.received"])
                                    .arg(format!("{channel}"))
                                    .arg(&body)
                                    .spawn();
                            }
                            if layout.sound {
                                let _ = std::process::Command::new("paplay")
                                    .arg("/usr/share/sounds/freedesktop/stereo/message-new-instant.oga")
                                    .spawn();
                            }
                        }
                    }
                }
                self.last_notified = self.messages.len();

                // Auto-connect servers on first tick
                let auto_connect: Vec<usize> = self.config
                    .servers
                    .iter()
                    .enumerate()
                    .filter(|(i, s)| s.auto_connect && self.servers.get(*i).map_or(false, |st| st.connection == ConnectionState::Disconnected))
                    .map(|(i, _)| i)
                    .collect();

                for idx in auto_connect {
                    let sc = &self.config.servers[idx];
                    let profile = self.config.resolve_profile(sc);
                    let conn_cfg = build_connection_config(sc, profile);
                    if let Some(s) = self.servers.get_mut(idx) {
                        s.connection = ConnectionState::Connecting;
                        s.current_nick = profile.nick.clone();
                    }
                    rebuild_nav(&mut self.nav, &self.servers);

                    let (cmd_tx, msg_rx) = spawn_connection(idx, conn_cfg);
                    self.cmd_txs.insert(idx, cmd_tx);
                    self.msg_rx = Some(msg_rx);
                }

                // Mark servers as Connected when we see the "Connected" message
                let server_info: Vec<(usize, String)> = self.servers.iter()
                    .map(|s| (s.idx, s.name.clone()))
                    .collect();
                let mut need_rebuild = false;
                for (idx, _name) in &server_info {
                    let connected = self.messages.iter().any(|m| {
                        m.server == *idx
                            && m.kind == MessageKind::Server
                            && m.text.starts_with("Connected")
                    });
                    if connected {
                        if let Some(s) = self.servers.get_mut(*idx) {
                            if s.connection == ConnectionState::Connecting {
                                s.connection = ConnectionState::Connected;
                                need_rebuild = true;
                            }
                        }
                    }
                }
                if need_rebuild {
                    rebuild_nav(&mut self.nav, &self.servers);
                }
            }

            Message::NavSelect(_) => {}

            Message::InputChanged(text) => {
                self.input = text;
            }

            Message::SendMessage => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return Task::none();
                }
                self.input.clear();

                // Determine server index from current selection
                let server_idx = match &self.selected {
                    Some(NavItem::Server(i)) | Some(NavItem::Channel(i, _)) => *i,
                    None => return Task::none(),
                };

                if text.starts_with('/') {
                    let current_ch = match &self.selected {
                        Some(NavItem::Channel(_, ch)) => Some(ch.clone()),
                        _ => None,
                    };
                    let new_selection = handle_slash_command(
                        server_idx,
                        &text,
                        current_ch,
                        &mut self.messages,
                        &mut self.cmd_txs,
                        &mut self.servers,
                    );
                    if let Some(ns) = new_selection {
                        self.selected = Some(ns);
                    }

                } else if let Some(NavItem::Channel(_, ch)) = &self.selected {
                    if let Some(tx) = self.cmd_txs.get(&server_idx) {
                        let _ = tx.send(IrcCommand::SendMsg(server_idx, ch.clone(), text));
                    }
                }
            }

            Message::ToggleContextPage(page) => {
                if self.core.window.show_context && self.context_page == page {
                    self.core.window.show_context = false;
                } else {
                    self.context_page = page;
                    self.core.window.show_context = true;
                }
            }

            Message::Connect(idx) => {
                if let Some(sc) = self.config.servers.get(idx).cloned() {
                    let profile = self.config.resolve_profile(&sc);
                    let conn_cfg = build_connection_config(&sc, profile);

                    if let Some(s) = self.servers.get_mut(idx) {
                        s.connection = ConnectionState::Connecting;
                        s.current_nick = profile.nick.clone();
                    }
                    rebuild_nav(&mut self.nav, &self.servers);

                    let (cmd_tx, msg_rx) = spawn_connection(idx, conn_cfg);
                    self.cmd_txs.insert(idx, cmd_tx);
                    self.msg_rx = Some(msg_rx);
                }
            }

            Message::Disconnect(idx) => {
                self.cmd_txs.remove(&idx);
                if let Some(tx) = self.cmd_txs.get(&idx) {
                    let _ = tx.send(IrcCommand::Disconnect);
                }
                if let Some(s) = self.servers.get_mut(idx) {
                    s.connection = ConnectionState::Disconnected;
                }
                rebuild_nav(&mut self.nav, &self.servers);
            }

            Message::AddServerDialog => {
                self.dialog_show = true;
                self.dialog_server = ServerConfig::default();
                self.dialog_channels_str = "#cosmic-chat".into();
                self.dialog_profile = "default".into();
            }

            Message::AddServer(cfg) => {
                self.dialog_show = false;
                let idx = self.config.servers.len();
                let nick = self.config.resolve_profile(&cfg).nick.clone();
                self.config.servers.push(cfg.clone());
                self.config.save();
                self.servers.push(ServerState {
                    idx,
                    name: cfg.name.clone(),
                    connection: ConnectionState::Disconnected,
                    channels: cfg.channels.clone(),
                    current_nick: nick,
                });
                rebuild_nav(&mut self.nav, &self.servers);
            }

            Message::Noop => {}
        }

        Task::none()
    }
}

// ── View helpers ────────────────────────────────────────────────────────────

impl CosmicChat {
    fn view_welcome(&self) -> Element<'_, Message> {
        let sp = cosmic::theme::spacing();

        let server_count = self.config.servers.len();
        let profile_count = self.config.profile.names().len();
        let body = format!(
            "Servers configured: {server_count}\nUser profiles: {profile_count}\n\nConnect to a server or add a new one."
        );

        widget::column::with_capacity(4)
            .push(widget::text::title2("COSMIC Chat"))
            .push(widget::text::body(body))
            .push(space::vertical().height(sp.space_m))
            .push(widget::button::suggested("+ Add Server").on_press(Message::AddServerDialog))
            .spacing(sp.space_s)
            .padding(sp.space_l)
            .align_x(Alignment::Center)
            .into()
    }

    fn view_server(&self, idx: usize) -> Element<'_, Message> {
        let sp = cosmic::theme::spacing();

        let server = match self.servers.get(idx) {
            Some(sv) => sv,
            None => return self.view_welcome(),
        };

        let sc = self.config.servers.get(idx);
        let profile_name = sc.map(|c| c.profile.as_str()).unwrap_or("default");
        let status = format!("{:?}", server.connection);

        let mut ch_list = widget::column::with_capacity(server.channels.len() + 1);
        for ch in &server.channels {
            ch_list = ch_list.push(widget::text::body(format!("  # {ch}")));
        }

        let is_connected = server.connection == ConnectionState::Connected;
        let action_btn: Element<'_, Message> = if is_connected {
            widget::button::standard("Disconnect")
                .on_press(Message::Disconnect(idx))
                .into()
        } else {
            widget::button::suggested("Connect")
                .on_press(Message::Connect(idx))
                .into()
        };

        widget::column::with_capacity(7)
            .push(widget::text::title3(&server.name))
            .push(widget::text::caption(format!(
                "Nick: {}  |  Profile: {profile_name}  |  {status}",
                server.current_nick,
            )))
            .push(space::vertical().height(sp.space_m))
            .push(widget::text::body("Channels:"))
            .push(ch_list.spacing(sp.space_xs))
            .push(space::vertical().height(sp.space_m))
            .push(action_btn)
            .spacing(sp.space_s)
            .padding(sp.space_m)
            .into()
    }

    fn view_channel(&self, server_idx: usize, channel: &str) -> Element<'_, Message> {
        let sp = cosmic::theme::spacing();
        let layout = &self.config.layout;

        let key = (server_idx, channel.to_string());

        // ── Topic bar ──────────────────────────────────────────────────
        let topic_text = self.channel_topics.get(&key).cloned().unwrap_or_default();
        let topic_bar = widget::container(
            if topic_text.is_empty() {
                widget::text::caption("No topic set")
            } else {
                widget::text::caption(format!("Topic: {topic_text}"))
            }
        )
        .width(Length::Fill)
        .padding([2, 8]);

        // ── Message area ───────────────────────────────────────────────
        let mut msgs: Vec<&IrcMessage> = self
            .messages
            .iter()
            .filter(|m| m.server == server_idx && m.target == *channel)
            .collect();

        if msgs.len() > layout.max_scrollback {
            let skip = msgs.len() - layout.max_scrollback;
            msgs = msgs[skip..].to_vec();
        }

        let mut msg_col = widget::column::with_capacity(msgs.len().max(1)).spacing(1);

        if msgs.is_empty() {
            msg_col = msg_col.push(widget::text::body("— end of /MOTD —"));
        } else {
            for m in &msgs {
                if !layout.show_join_part
                    && matches!(m.kind, MessageKind::Join | MessageKind::Part | MessageKind::Quit)
                {
                    continue;
                }

                // Build IRC-style line: [HH:MM] <nick> message
                let ts = if layout.show_timestamps {
                    format!("[{}]", &m.time[..5.min(m.time.len())])
                } else {
                    String::new()
                };

                let (prefix, body, is_event) = match m.kind {
                    MessageKind::Chat => {
                        let nick = m.sender.as_deref().unwrap_or("?");
                        (format!("<{nick}>"), m.text.clone(), false)
                    }
                    MessageKind::Action => {
                        let nick = m.sender.as_deref().unwrap_or("?");
                        (format!("* {nick}"), m.text.clone(), false)
                    }
                    MessageKind::Join => (String::new(), format!("→ {}", m.text), true),
                    MessageKind::Part => (String::new(), format!("← {}", m.text), true),
                    MessageKind::Quit => (String::new(), format!("← {}", m.text), true),
                    MessageKind::Notice => { let t = &m.text; (String::new(), format!("— {t}"), true) },
                    MessageKind::Topic => { let t = &m.text; (String::new(), format!("— {t}"), true) },
                    MessageKind::Server => { let t = &m.text; (String::new(), format!("— {t}"), true) },
                };

                let element: Element<'_, Message> = if is_event {
                    widget::row::with_capacity(2)
                        .push(widget::text::caption(ts))
                        .push(widget::text::body(body))
                        .spacing(sp.space_xs)
                        .into()
                } else {
                    widget::row::with_capacity(3)
                        .push(widget::text::caption(ts))
                        .push(widget::text::body(prefix))
                        .push(widget::text::body(body))
                        .spacing(sp.space_xs)
                        .into()
                };

                msg_col = msg_col.push(element);
            }
        }

        let scroll = widget::scrollable(msg_col)
            .width(Length::Fill)
            .height(Length::Fill);

        // ── User list ──────────────────────────────────────────────────
        let users = self.channel_users.get(&key);
        let user_count = users.map(|u| u.len()).unwrap_or(0);
        let mut user_col = widget::column::with_capacity(user_count.max(1)).spacing(1);

        user_col = user_col.push(
            widget::text::caption(format!("Users ({user_count})"))
        );

        if let Some(ulist) = users {
            for nick in ulist {
                user_col = user_col.push(widget::text::body(nick.clone()));
            }
        }

        let user_panel = widget::scrollable(user_col)
            .width(Length::Fixed(140.0))
            .height(Length::Fill);

        // ── Input bar ──────────────────────────────────────────────────
        let input_bar = widget::row::with_capacity(2)
            .push({
                widget::text_input("Type a message or /command...", &self.input)
                    .on_input(Message::InputChanged)
                    .on_submit(|_| Message::SendMessage)
                    .width(Length::Fill)
            })
            .push(
                widget::button::suggested("Send")
                    .on_press(Message::SendMessage),
            )
            .spacing(sp.space_s)
            .align_y(Alignment::Center);

        // ── Assemble layout ────────────────────────────────────────────
        // Header: channel name + topic bar
        let header = widget::column::with_capacity(2)
            .push(
                widget::container(widget::text::title4(format!("# {channel}")))
                    .padding([2, 8])
            )
            .push(topic_bar)
            .spacing(0);

        // Body: message area + user list
        let body = widget::row::with_capacity(2)
            .push(scroll)
            .push(user_panel)
            .spacing(2);

        // Full layout
        widget::column::with_capacity(3)
            .push(header)
            .push(body)
            .push(
                widget::container(input_bar)
                    .padding([4, 8])
            )
            .spacing(0)
            .into()
    }

    fn view_add_server_dialog(&self) -> Element<'_, Message> {
        let sp = cosmic::theme::spacing();

        widget::column::with_capacity(12)
            .push(widget::text::title3("Add IRC Server"))
            .push(space::vertical().height(sp.space_s))
            .push(widget::text_input("Server name", &self.dialog_server.name)
                .on_input(|_| Message::Noop))
            .push(widget::text_input("Host", &self.dialog_server.host)
                .on_input(|_| Message::Noop))
            .push(widget::text_input("Profile (user identity)", &self.dialog_profile)
                .on_input(|_| Message::Noop))
            .push(widget::text_input("Channels (comma-separated)", &self.dialog_channels_str)
                .on_input(|_| Message::Noop))
            .push(widget::text::caption("Port: 6697 (TLS)  |  Auto-connect: yes"))
            .push(space::vertical().height(sp.space_m))
            .push(
                widget::row::with_capacity(2)
                    .push(widget::button::suggested("Add").on_press(
                        Message::AddServer(self.dialog_server.clone())
                    ))
                    .push(widget::button::standard("Cancel").on_press(Message::Noop))
                    .spacing(sp.space_s),
            )
            .spacing(sp.space_s)
            .padding(sp.space_l)
            .width(420)
            .into()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn build_connection_config(sc: &ServerConfig, profile: &UserProfile) -> ConnectionConfig {
    ConnectionConfig {
        host: sc.host.clone(),
        port: sc.port,
        tls: sc.tls,
        nick: profile.nick.clone(),
        user: profile.user.clone(),
        realname: profile.realname.clone(),
        password: sc.password.clone(),
        channels: sc.channels.clone(),
    }
}

fn handle_slash_command(
    server_idx: usize,
    text: &str,
    current_channel: Option<String>,
    messages: &mut Vec<IrcMessage>,
    cmd_txs: &mut HashMap<usize, mpsc::UnboundedSender<IrcCommand>>,
    servers: &mut Vec<ServerState>,
) -> Option<NavItem> {
    let body = &text[1..]; // strip leading /
    let mut parts = body.splitn(3, ' ');
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let arg1 = parts.next().map(|s| s.to_string()).unwrap_or_default();
    let arg2 = parts.next().map(|s| s.to_string()).unwrap_or_default();

    let local_echo = |messages: &mut Vec<IrcMessage>, text: &str| {
        messages.push(IrcMessage {
            server: server_idx,
            target: String::new(),
            sender: None,
            text: text.to_string(),
            kind: MessageKind::Server,
            time: crate::irc_client::now_hhmmss(),
        });
    };

    match cmd.as_str() {
        "join" | "j" => {
            if arg1.is_empty() {
                local_echo(messages, "Usage: /join #channel");
                return None;
            }
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::Join(server_idx, arg1.clone()));
                local_echo(messages, &format!("Joining {arg1}…"));
            }
        }

        "part" | "leave" => {
            let ch = if arg1.starts_with('#') {
                arg1.clone()
            } else if let Some(ref ch) = current_channel {
                ch.clone()
            } else {
                local_echo(messages, "Usage: /part [#channel] [reason]");
                return None;
            };
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::Part(server_idx, ch.clone()));
                local_echo(messages, &format!("Leaving {ch}"));
            }
            // Clear selection if leaving current channel
            return None;
        }

        "quit" | "disconnect" => {
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::Disconnect);
            }
            cmd_txs.remove(&server_idx);
            if let Some(s) = servers.get_mut(server_idx) {
                s.connection = ConnectionState::Disconnected;
            }
            local_echo(messages, "Disconnected");
            return None;
        }

        "me" => {
            let action_text = if arg2.is_empty() {
                arg1.clone()
            } else {
                format!("{arg1} {arg2}")
            };
            if action_text.is_empty() {
                local_echo(messages, "Usage: /me <action>");
                return None;
            }
            // Figure out target — current channel or arg1 if it looks like a nick
            let target = if let Some(ref ch) = current_channel {
                ch.clone()
            } else {
                local_echo(messages, "Select a channel first for /me");
                return None;
            };
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::SendAction(server_idx, target, action_text));
            }
        }

        "msg" | "query" => {
            if arg1.is_empty() || arg2.is_empty() {
                local_echo(messages, "Usage: /msg <nick> <message>");
                return None;
            }
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::SendMsg(server_idx, arg1.clone(), arg2.clone()));
            }
            local_echo(messages, &format!("-> *{arg1}* {arg2}"));
        }

        "nick" => {
            if arg1.is_empty() {
                local_echo(messages, "Usage: /nick <newnick>");
                return None;
            }
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::Nick(server_idx, arg1.clone()));
            }
            if let Some(s) = servers.get_mut(server_idx) {
                s.current_nick = arg1.clone();
            }
            local_echo(messages, &format!("Changing nick to {arg1}…"));
        }

        "topic" => {
            let ch = if arg1.starts_with('#') {
                let topic = arg2;
                if let Some(tx) = cmd_txs.get(&server_idx) {
                    let _ = tx.send(IrcCommand::Raw(server_idx, format!("TOPIC {arg1} :{topic}")));
                }
                return None;
            } else if let Some(ref ch) = current_channel {
                ch.clone()
            } else {
                local_echo(messages, "Usage: /topic [#channel] [new topic]");
                return None;
            };
            let topic = format!("{arg1} {arg2}").trim().to_string();
            if topic.is_empty() {
                // View topic
                if let Some(tx) = cmd_txs.get(&server_idx) {
                    let _ = tx.send(IrcCommand::Raw(server_idx, format!("TOPIC {ch}")));
                }
            } else {
                if let Some(tx) = cmd_txs.get(&server_idx) {
                    let _ = tx.send(IrcCommand::Raw(server_idx, format!("TOPIC {arg1} :{topic}")));
                }
            }
        }

        "whois" => {
            if arg1.is_empty() {
                local_echo(messages, "Usage: /whois <nick>");
                return None;
            }
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::Raw(server_idx, format!("WHOIS {arg1}")));
            }
        }

        "raw" | "quote" => {
            let raw = format!("{arg1} {arg2}").trim().to_string();
            if raw.is_empty() {
                local_echo(messages, "Usage: /raw <irc command>");
                return None;
            }
            if let Some(tx) = cmd_txs.get(&server_idx) {
                let _ = tx.send(IrcCommand::Raw(server_idx, raw.clone()));
            }
            local_echo(messages, &format!("-> {raw}"));
        }

        _ => {
            local_echo(messages, &format!("Unknown command: /{cmd}"));
        }
    }

    // Keep selection unchanged for most commands
    current_channel.map(|ch| NavItem::Channel(server_idx, ch))
}


fn rebuild_nav(nav: &mut nav_bar::Model, servers: &[ServerState]) {
    *nav = nav_bar::Model::default();

    for server in servers {
        let icon = match server.connection {
            ConnectionState::Connected => "network-transmit-symbolic",
            ConnectionState::Connecting => "emblem-synchronizing-symbolic",
            ConnectionState::Error(_) => "dialog-error-symbolic",
            ConnectionState::Disconnected => "network-offline-symbolic",
        };

        nav.insert()
            .text(server.name.clone())
            .icon(widget::icon::from_name(icon))
            .data::<NavItem>(NavItem::Server(server.idx));

        for ch in &server.channels {
            nav.insert()
                .text(format!("  # {ch}"))
                .icon(widget::icon::from_name("user-available-symbolic"))
                .data::<NavItem>(NavItem::Channel(server.idx, ch.clone()));
        }
    }
}

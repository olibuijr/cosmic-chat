use std::collections::HashMap;

use cosmic::app::{Core, Task, context_drawer};
use cosmic::iced::{Alignment, Length, Subscription};
use cosmic::widget::{self, nav_bar, space};
use cosmic::Element;
use tokio::sync::mpsc;

use crate::config::{Config, ServerConfig, UserProfile};
use crate::irc_client::{
    ConnectionConfig, ConnectionState, IrcCommand, IrcMessage, MessageKind, ServerState,
    spawn_connection,
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
                        if msg.kind == MessageKind::Join {
                            if let Some(s) = self.servers.get_mut(msg.server) {
                                if !s.channels.contains(&msg.target) {
                                    s.channels.push(msg.target.clone());
                                    s.channels.sort();
                                }
                            }
                        }
                        self.messages.push(msg);
                    }
                }

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

                if let Some(NavItem::Channel(server_idx, channel)) = &self.selected {
                    if let Some(tx) = self.cmd_txs.get(server_idx) {
                        let _ = tx.send(IrcCommand::SendMsg(
                            *server_idx,
                            channel.clone(),
                            text,
                        ));
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

        let mut msgs: Vec<&IrcMessage> = self
            .messages
            .iter()
            .filter(|m| m.server == server_idx && m.target == *channel)
            .collect();

        // Apply max scrollback
        if msgs.len() > layout.max_scrollback {
            let skip = msgs.len() - layout.max_scrollback;
            msgs = msgs[skip..].to_vec();
        }

        let mut msg_col = widget::column::with_capacity(msgs.len().max(1)).spacing(2);

        if msgs.is_empty() {
            msg_col = msg_col.push(widget::text::body("No messages yet."));
        } else {
            for m in &msgs {
                // Respect show_join_part setting
                if !layout.show_join_part
                    && matches!(m.kind, MessageKind::Join | MessageKind::Part | MessageKind::Quit)
                {
                    continue;
                }

                let display_text = match m.kind {
                    MessageKind::Chat => match &m.sender {
                        Some(nick) => format!("<{nick}> {}", m.text),
                        None => m.text.clone(),
                    },
                    MessageKind::Action => match &m.sender {
                        Some(nick) => format!("* {nick} {}", m.text),
                        None => format!("* {}", m.text),
                    },
                    MessageKind::Join => format!("→ {}", m.text),
                    MessageKind::Part | MessageKind::Quit => format!("← {}", m.text),
                    MessageKind::Notice => format!("[notice] {}", m.text),
                    MessageKind::Topic | MessageKind::Server => m.text.clone(),
                };

                let mut line = widget::row::with_capacity(2).spacing(sp.space_s);

                if layout.show_timestamps {
                    line = line.push(widget::text::caption(&m.time));
                }
                line = line.push(widget::text::body(display_text));

                msg_col = msg_col.push(line);
            }
        }

        let scroll = widget::scrollable(msg_col).width(Length::Fill);

        widget::column::with_capacity(3)
            .push(widget::text::title4(format!("# {channel}")))
            .push(space::vertical().height(sp.space_s))
            .push(scroll)
            .push(space::vertical().height(6))
            .push(
                widget::row::with_capacity(2)
                    .push({
                        widget::text_input("Type a message...", &self.input)
                            .on_input(Message::InputChanged)
                            .on_submit(|_| Message::SendMessage)
                            .width(Length::Fill)
                    })
                    .push(
                        widget::button::suggested("Send")
                            .on_press(Message::SendMessage),
                    )
                    .spacing(sp.space_s)
                    .align_y(Alignment::Center),
            )
            .spacing(0)
            .padding([sp.space_s, sp.space_m])
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

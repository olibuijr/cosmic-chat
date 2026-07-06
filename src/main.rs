mod app;
mod config;
mod irc_client;

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn"),
    )
    .init();

    let settings = cosmic::app::Settings::default()
        .size_limits(cosmic::iced::Limits::NONE.min_width(640.0).min_height(400.0))
        .size(cosmic::iced::Size::new(900.0, 640.0));

    cosmic::app::run::<app::CosmicChat>(settings, ())?;
    Ok(())
}

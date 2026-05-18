use clap::Parser;

mod cli;
mod error;
mod io;
mod model;
mod response;

fn main() {
    let cli = cli::Cli::parse();
    let result = cli.run();
    if let Err(e) = result {
        let envelope = response::ResponseEnvelope::<()>::from_error(&e);
        println!("{}", serde_json::to_string(&envelope).unwrap_or_else(|_| {
            r#"{"ok":false,"error":{"code":"INTERNAL","message":"Failed to serialize error","details":{}}}"#.to_string()
        }));
        std::process::exit(1);
    }
}

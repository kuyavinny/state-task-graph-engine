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
        let envelope = response::ResponseEnvelope::<()>::from_error(&e, None);
        let json = serde_json::to_string(&envelope)
            .unwrap_or_else(|_| response::ResponseEnvelope::internal_fallback_json());
        println!("{}", json);
        std::process::exit(1);
    }
}

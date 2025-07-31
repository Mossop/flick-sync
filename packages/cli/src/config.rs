use clap::{Args, builder::TypedValueParser};
use flick_sync::{FlickSync, OutputStyle};
use tracing::instrument;

use crate::{Result, Runnable, console::Console};

#[derive(Args)]
pub struct SetOutputStyle {
    /// The output style to use.
    #[arg(
        value_parser = clap::builder::PossibleValuesParser::new(["minimal", "standardized"])
            .map(|s| s.parse::<OutputStyle>().unwrap()),
    )]
    style: OutputStyle,
}

impl Runnable for SetOutputStyle {
    #[instrument(name = "SetOutputStyle", skip_all)]
    async fn run(self, flick_sync: FlickSync, _console: Console) -> Result {
        flick_sync.update_output_style(self.style).await?;
        flick_sync.prune_root().await;

        Ok(())
    }
}

use std::sync::OnceLock;

use indicatif::ProgressStyle;

pub fn main_progress_style() -> &'static ProgressStyle {
    static MEM: OnceLock<ProgressStyle> = OnceLock::new();
    MEM.get_or_init(|| {
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap()
        .progress_chars("█▓▒░▫")
    })
}

pub fn spin_progress_style() -> &'static ProgressStyle {
    static MEM: OnceLock<ProgressStyle> = OnceLock::new();
    MEM.get_or_init(|| ProgressStyle::default_spinner().tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏◇"))
}

pub fn item_progress_style() -> &'static ProgressStyle {
    static MEM: OnceLock<ProgressStyle> = OnceLock::new();
    MEM.get_or_init(||
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {decimal_bytes:>12}/{decimal_total_bytes:12} {msg}",
        )
        .unwrap()
        .progress_chars("█▇▆▅▄▃▂▁  ")
    )
}

pub fn item_success_style() -> &'static ProgressStyle {
    static MEM: OnceLock<ProgressStyle> = OnceLock::new();
    MEM.get_or_init(||
        ProgressStyle::with_template(
            "[{elapsed_precise:.dim}] {bar:40.green.dim/green.dim}            ↓ {decimal_total_bytes:12.dim} {msg:.dim}",
        )
        .unwrap()
        .progress_chars("█▇▆▅▄▃▂▁  ")
    )
}

pub fn item_failure_style() -> &'static ProgressStyle {
    static MEM: OnceLock<ProgressStyle> = OnceLock::new();
    MEM.get_or_init(||
        ProgressStyle::with_template(
            "[{elapsed_precise:.red.dim}] {bar:40.red.dim/red.dim}            ↓ {decimal_total_bytes:12.red.dim} {msg:.red.dim}",
        )
        .unwrap()
        .progress_chars("█▇▆▅▄▃▂▁  ")
    )
}

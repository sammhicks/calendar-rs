use std::path::PathBuf;

#[derive(clap::Parser)]
struct Args {
    #[clap()]
    input_filename: PathBuf,
    #[clap()]
    output_filename: PathBuf,
}

fn main() {
    let Args {
        input_filename,
        output_filename,
    } = clap::Parser::parse();

    let output = std::fs::read_to_string(&input_filename)
        .unwrap_or_else(|_| panic!("Failed to read {}", input_filename.display()))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (date, title) = line.split_once('\t')?;

            let (day, month) = date.split_once('-').expect("- not found");

            let day = day.trim().trim_start_matches('0');
            let month = month.trim();
            let title = title.trim();

            (!title.is_empty()).then(|| format!("{day} {month} {title}\r\n"))
        })
        .collect::<String>();

    std::fs::write(&output_filename, output)
        .unwrap_or_else(|_| panic!("Failed to write output to {}", output_filename.display()));
}

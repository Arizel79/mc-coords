use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum World {
    Overworld,
    Nether,
}

impl World {
    fn parse(s: &str) -> Option<World> {
        match s.to_ascii_lowercase().as_str() {
            "n" | "nether" => Some(World::Nether),
            "o" | "ow" | "overworld" => Some(World::Overworld),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            World::Overworld => "overworld",
            World::Nether => "nether",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            World::Overworld => "Overworld",
            World::Nether => "Nether",
        }
    }

    fn target(self) -> World {
        match self {
            World::Overworld => World::Nether,
            World::Nether => World::Overworld,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Coords {
    x: i64,
    y: Option<i64>,
    z: i64,
}

impl Coords {
    fn parse(tokens: &[&str]) -> Result<Coords, String> {
        let parts: Vec<&str> = if tokens.len() == 1 && tokens[0].contains(',') {
            tokens[0]
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            tokens.to_vec()
        };

        let nums: Vec<i64> = parts
            .iter()
            .map(|p| {
                p.trim()
                    .parse()
                    .map_err(|_| format!("invalid coordinate: {p:?}"))
            })
            .collect::<Result<_, _>>()?;

        match nums.len() {
            2 => Ok(Coords {
                x: nums[0],
                y: None,
                z: nums[1],
            }),
            3 => Ok(Coords {
                x: nums[0],
                y: Some(nums[1]),
                z: nums[2],
            }),
            n => Err(format!("expected 2 or 3 coordinates, got {n}")),
        }
    }

    fn convert(self, from: World) -> Result<Coords, String> {
        match from {
            World::Overworld => Ok(Coords {
                x: self.x / 8,
                y: self.y,
                z: self.z / 8,
            }),
            World::Nether => {
                let x = self
                    .x
                    .checked_mul(8)
                    .ok_or_else(|| format!("X * 8 overflows i64: {}", self.x))?;
                let z = self
                    .z
                    .checked_mul(8)
                    .ok_or_else(|| format!("Z * 8 overflows i64: {}", self.z))?;
                Ok(Coords {
                    x,
                    y: self.y,
                    z,
                })
            }
        }
    }

    fn parts(self, no_y: bool) -> Vec<i64> {
        match (self.y, no_y) {
            (Some(y), false) => vec![self.x, y, self.z],
            _ => vec![self.x, self.z],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Item {
    from: World,
    coords: Coords,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OutputFormat {
    Space,
    Comma,
}

impl OutputFormat {
    fn parse(s: &str) -> Result<OutputFormat, String> {
        match s {
            "space-sep" => Ok(OutputFormat::Space),
            "comma-sep" => Ok(OutputFormat::Comma),
            _ => Err(format!(
                "unknown output format: {s:?} (expected space-sep or comma-sep)"
            )),
        }
    }

    fn sep(self) -> &'static str {
        match self {
            OutputFormat::Space => " ",
            OutputFormat::Comma => ",",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum McVersion {
    Old,
    New,
}

impl McVersion {
    fn parse(s: &str) -> Result<McVersion, String> {
        match s {
            "old" => Ok(McVersion::Old),
            "new" => Ok(McVersion::New),
            _ => Err(format!(
                "unknown MC version: {s:?} (expected old or new)"
            )),
        }
    }

    fn xz_limit(self) -> i64 {
        match self {
            McVersion::Old => 30_000_000,
            McVersion::New => 29_999_984,
        }
    }

    fn name(self) -> &'static str {
        match self {
            McVersion::Old => "old",
            McVersion::New => "new",
        }
    }

    fn y_range(self, dim: World) -> (i64, i64) {
        match (self, dim) {
            (McVersion::Old, World::Overworld) => (0, 255),
            (McVersion::Old, World::Nether) => (0, 127),
            (McVersion::New, _) => (-64, 319),
        }
    }

    fn is_valid(self, dim: World, c: &Coords) -> bool {
        let xz = self.xz_limit();
        let (y_lo, y_hi) = self.y_range(dim);
        c.x.abs() <= xz && c.z.abs() <= xz && c.y.is_none_or(|y| y >= y_lo && y <= y_hi)
    }
}

#[derive(Clone)]
struct ConvertOpts {
    input: Option<String>,
    output: Option<String>,
    no_y: bool,
    add_dimension_to_output: bool,
    add_from_dimension: bool,
    output_format: OutputFormat,
    mc_version: McVersion,
    ignore_invalid_coords: bool,
}

#[derive(Clone)]
struct ValidateOpts {
    input: Option<String>,
    output: Option<String>,
    mc_version: McVersion,
    dimension: Option<World>,
}

fn parse_line(line: &str, default_world: Option<World>) -> Result<Item, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() {
        return Err("empty line".into());
    }

    if let Some(world) = World::parse(tokens[0]) {
        let rest = &tokens[1..];
        if rest.is_empty() {
            return Err("missing coordinates".into());
        }
        let coords = Coords::parse(rest)?;
        Ok(Item { from: world, coords })
    } else {
        match default_world {
            Some(world) => {
                let coords = Coords::parse(&tokens)?;
                Ok(Item { from: world, coords })
            }
            None => Err(format!("unknown world: {:?}", tokens[0])),
        }
    }
}

fn parse_validate_line(line: &str, default_dim: World) -> Result<(World, Coords), String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() {
        return Err("empty line".into());
    }
    match World::parse(tokens[0]) {
        Some(dim) => Ok((dim, Coords::parse(&tokens[1..])?)),
        None => Ok((default_dim, Coords::parse(&tokens)?)),
    }
}

fn validity_reason(version: McVersion, dim: World, c: &Coords) -> Option<String> {
    let xz = version.xz_limit();
    if c.x.abs() > xz || c.z.abs() > xz {
        return Some(format!("X/Z out of range [-{xz}, {xz}]"));
    }
    let (y_lo, y_hi) = version.y_range(dim);
    if let Some(y) = c.y
        && (y < y_lo || y > y_hi)
    {
        return Some(format!("Y out of range [{y_lo}, {y_hi}]"));
    }
    None
}

fn fmt_coords(c: &Coords) -> String {
    match c.y {
        Some(y) => format!("{} {} {}", c.x, y, c.z),
        None => format!("{} {}", c.x, c.z),
    }
}

fn render(item: &Item, opts: &ConvertOpts) -> Result<String, String> {
    let to_coords = item.coords.convert(item.from)?;
    let target = item.from.target();
    let parts = |c: &Coords| -> String {
        c.parts(opts.no_y)
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(opts.output_format.sep())
    };

    if opts.add_from_dimension {
        Ok(format!(
            "{} {} -> {} {}",
            item.from.display_name(),
            parts(&item.coords),
            target.display_name(),
            parts(&to_coords)
        ))
    } else if opts.add_dimension_to_output {
        Ok(format!("{} {}", target.name(), parts(&to_coords)))
    } else {
        Ok(parts(&to_coords))
    }
}

fn render_items(items: &[Item], opts: &ConvertOpts) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    for item in items {
        if let Some(reason) = validity_reason(opts.mc_version, item.from, &item.coords)
            && !opts.ignore_invalid_coords
        {
            return Err(format!(
                "invalid coords for {} ({}) with mc-version {}: {}",
                item.from.name(),
                fmt_coords(&item.coords),
                opts.mc_version.name(),
                reason
            ));
        }
        output.push(render(item, opts)?);
    }
    Ok(output)
}

fn parse_convert_flags(args: &[String]) -> Result<(ConvertOpts, Vec<String>), String> {
    let mut opts = ConvertOpts {
        input: None,
        output: None,
        no_y: false,
        add_dimension_to_output: false,
        add_from_dimension: false,
        output_format: OutputFormat::Space,
        mc_version: McVersion::New,
        ignore_invalid_coords: false,
    };
    let mut positional: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_convert_usage_to(&mut io::stdout());
                process::exit(0);
            }
            "-i" => opts.input = Some(it.next().ok_or("missing value for -i")?.clone()),
            "-o" => opts.output = Some(it.next().ok_or("missing value for -o")?.clone()),
            "--no-y" => opts.no_y = true,
            "--add-dimension-to-output" => opts.add_dimension_to_output = true,
            "--add-from-dimension" => opts.add_from_dimension = true,
            "--mc-version" => {
                let version = it.next().ok_or("missing value for --mc-version")?;
                opts.mc_version = McVersion::parse(version)?;
            }
            "--ignore-invalid-coords" => opts.ignore_invalid_coords = true,
            "--output-format" => {
                let fmt = it.next().ok_or("missing value for --output-format")?;
                opts.output_format = OutputFormat::parse(fmt)?;
            }
            other => positional.push(other.to_string()),
        }
    }

    Ok((opts, positional))
}

fn parse_validate_flags(args: &[String]) -> Result<(ValidateOpts, Vec<String>), String> {
    let mut opts = ValidateOpts {
        input: None,
        output: None,
        mc_version: McVersion::New,
        dimension: None,
    };
    let mut positional: Vec<String> = Vec::new();
    let mut mc_version: Option<McVersion> = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_validate_usage_to(&mut io::stdout());
                process::exit(0);
            }
            "-i" => opts.input = Some(it.next().ok_or("missing value for -i")?.clone()),
            "-o" => opts.output = Some(it.next().ok_or("missing value for -o")?.clone()),
            "--mc-version" => {
                let version = it.next().ok_or("missing value for --mc-version")?;
                mc_version = Some(McVersion::parse(version)?);
            }
            "--dimension" => {
                let name = it.next().ok_or("missing value for --dimension")?;
                opts.dimension = Some(
                    World::parse(name)
                        .ok_or(format!("invalid dimension: {name:?} (expected n/nether/o/ow/overworld)"))?,
                );
            }
            other => positional.push(other.to_string()),
        }
    }

    opts.mc_version = mc_version.ok_or("missing --mc-version old|new")?;
    Ok((opts, positional))
}

fn split_world(positional: &[String]) -> Result<(Option<World>, Vec<String>), String> {
    let mut world = None;
    let mut coords = Vec::new();
    for token in positional {
        if let Some(w) = World::parse(token) {
            if world.is_some() {
                return Err(format!("unexpected world: {token:?}"));
            }
            world = Some(w);
        } else {
            coords.push(token.clone());
        }
    }
    Ok((world, coords))
}

fn read_lines(source: &Option<String>) -> Result<Vec<String>, String> {
    match source {
        None => read_stdin(),
        Some(p) if p == "-" => read_stdin(),
        Some(path) => fs::read_to_string(path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .map(|s| s.lines().map(String::from).collect()),
    }
}

fn read_stdin() -> Result<Vec<String>, String> {
    let stdin = io::stdin();
    stdin
        .lock()
        .lines()
        .collect::<io::Result<Vec<_>>>()
        .map_err(|e| format!("cannot read stdin: {e}"))
}

fn write_output(sink: &Option<String>, lines: &[String]) -> Result<(), String> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut content = lines.join("\n");
    content.push('\n');
    match sink {
        None => write_stdout(&content),
        Some(p) if p == "-" => write_stdout(&content),
        Some(path) => fs::write(path, content).map_err(|e| format!("cannot write {path}: {e}")),
    }
}

fn write_stdout(content: &str) -> Result<(), String> {
    print!("{content}");
    io::stdout()
        .flush()
        .map_err(|e| format!("cannot write stdout: {e}"))
}

fn run_convert(args: &[String]) -> Result<(), String> {
    let (opts, positional) = parse_convert_flags(args)?;
    let (world, coord_tokens) = split_world(&positional)?;

    let items: Vec<Item> = if !coord_tokens.is_empty() {
        let world = world.ok_or("missing source world (use n/nether/o/ow/overworld)")?;
        let refs: Vec<&str> = coord_tokens.iter().map(String::as_str).collect();
        let coords = Coords::parse(&refs)?;
        vec![Item { from: world, coords }]
    } else {
        let lines = read_lines(&opts.input)?;
        let mut items = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            items.push(parse_line(&line, world)?);
        }
        if items.is_empty() {
            return Err("no input".into());
        }
        items
    };

    let output = match render_items(&items, &opts) {
        Ok(out) => out,
        Err(msg) => {
            eprintln!("error: {msg}");
            process::exit(1);
        }
    };
    write_output(&opts.output, &output)
}

fn run_validate(args: &[String]) -> Result<(), String> {
    let (opts, positional) = parse_validate_flags(args)?;
    let (pos_dim, coord_tokens) = split_world(&positional)?;

    let default_dim = match (opts.dimension, pos_dim) {
        (Some(flag), Some(pos)) if flag != pos => Err(format!(
            "conflicting dimensions: --dimension {:?} and positional {:?}",
            flag.name(),
            pos.name()
        ))?,
        (Some(flag), _) => flag,
        (None, pos) => pos.unwrap_or(World::Overworld),
    };

    let coords_list: Vec<(World, Coords)> = if !coord_tokens.is_empty() {
        let refs: Vec<&str> = coord_tokens.iter().map(String::as_str).collect();
        vec![(default_dim, Coords::parse(&refs)?)]
    } else {
        let lines = read_lines(&opts.input)?;
        let mut coords_list = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            coords_list.push(parse_validate_line(&line, default_dim)?);
        }
        if coords_list.is_empty() {
            return Err("no input".into());
        }
        coords_list
    };

    let output: Vec<String> = coords_list
        .iter()
        .map(|(dim, c)| {
            if opts.mc_version.is_valid(*dim, c) {
                "1".to_string()
            } else {
                "0".to_string()
            }
        })
        .collect();
    write_output(&opts.output, &output)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        process::exit(1);
    }

    match args[0].as_str() {
        "-h" | "--help" | "help" => {
            print_usage_to(&mut io::stdout());
            process::exit(0);
        }
        "convert" => exit_on_err(run_convert(&args[1..]), print_convert_usage),
        "validate" => exit_on_err(run_validate(&args[1..]), print_validate_usage),
        other => {
            eprintln!("error: unknown command: {other:?}");
            print_usage();
            process::exit(1);
        }
    }
}

fn exit_on_err(result: Result<(), String>, usage: fn()) {
    if let Err(e) = result {
        eprintln!("error: {e}");
        usage();
        process::exit(1);
    }
}

fn print_usage() {
    print_usage_to(&mut io::stderr());
}

fn print_usage_to(w: &mut dyn Write) {
    writeln!(w, "usage: mccoords <command> [OPTIONS]").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "commands:").unwrap();
    writeln!(w, "  convert   convert coordinates between the overworld and the nether").unwrap();
    writeln!(w, "  validate  check coordinates against the world limits of an MC version").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "run 'mccoords <command> --help' for details").unwrap();
}

fn print_convert_usage() {
    print_convert_usage_to(&mut io::stderr());
}

fn print_convert_usage_to(w: &mut dyn Write) {
    writeln!(w, "usage: mccoords convert [OPTIONS] [WORLD] [X [Y] Z]").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "converts Minecraft coordinates between the overworld and the nether").unwrap();
    writeln!(w, "  overworld -> nether:  X/8, Z/8   nether -> overworld:  X*8, Z*8   (Y is kept)").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "WORLD aliases: n, nether, o, ow, overworld (case-insensitive)").unwrap();
    writeln!(w, "coordinates: 'X Y Z', 'X Z', 'X,Y,Z', 'X,Z' (2 numbers = X Z)").unwrap();
    writeln!(w, "input format is auto-detected; --output-format affects output only").unwrap();
    writeln!(w, "coordinates are validated against the world limits of --mc-version (default: new);").unwrap();
    writeln!(w, "invalid coordinates make convert exit with an error").unwrap();
    writeln!(w, "  --ignore-invalid-coords converts them anyway, without errors").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "without coordinates in the arguments, lines are read from stdin (-i):").unwrap();
    writeln!(w, "  each line: [WORLD] coordinates   (WORLD needed unless given as an argument)").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "options:").unwrap();
    writeln!(w, "  -i PATH                        read input from PATH ('-' means stdin, default)").unwrap();
    writeln!(w, "  -o PATH                        write output to PATH ('-' means stdout, default)").unwrap();
    writeln!(w, "  --no-y                         omit Y from the output").unwrap();
    writeln!(w, "  --add-dimension-to-output      prefix output with the target world name").unwrap();
    writeln!(w, "  --add-from-dimension           print: FROM coords -> TO coords with world names").unwrap();
    writeln!(w, "  --output-format FORMAT         space-sep (default) or comma-sep").unwrap();
    writeln!(w, "  --mc-version old|new           MC version for coordinate limits (default: new)").unwrap();
    writeln!(w, "  --ignore-invalid-coords       convert out-of-bounds coordinates anyway").unwrap();
    writeln!(w, "  -h, --help                     show this help").unwrap();
}

fn print_validate_usage() {
    print_validate_usage_to(&mut io::stderr());
}

fn print_validate_usage_to(w: &mut dyn Write) {
    writeln!(w, "usage: mccoords validate [OPTIONS] [DIMENSION] [X [Y] Z]").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "checks coordinates against the world limits of an MC version").unwrap();
    writeln!(w, "prints 1 (valid) or 0 (invalid) per coordinate set").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "DIMENSION (default: overworld): n, nether, o, ow, overworld").unwrap();
    writeln!(w, "coordinates: 'X Y Z', 'X Z', 'X,Y,Z', 'X,Z' (2 numbers = X Z)").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "without coordinates in the arguments, lines are read from stdin (-i)").unwrap();
    writeln!(w, "each line may start with a DIMENSION, overriding the default").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "world limits by version (X/Z are the same in both dimensions):").unwrap();
    writeln!(w, "  old  overworld  X/Z in [-30,000,000, 30,000,000],  Y in [0, 255]").unwrap();
    writeln!(w, "  old  nether     X/Z in [-30,000,000, 30,000,000],  Y in [0, 127]").unwrap();
    writeln!(w, "  new  both       X/Z in [-29,999,984, 29,999,984],  Y in [-64, 319]").unwrap();
    writeln!(w, "  (without a Y coordinate, only X/Z are checked)").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "options:").unwrap();
    writeln!(w, "  -i PATH              read input from PATH ('-' means stdin, default)").unwrap();
    writeln!(w, "  -o PATH              write output to PATH ('-' means stdout, default)").unwrap();
    writeln!(w, "  --mc-version old|new MC version whose limits to check (required)").unwrap();
    writeln!(w, "  --dimension NAME     dimension to check (n/nether/o/ow/overworld)").unwrap();
    writeln!(w, "  -h, --help           show this help").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_opts() -> ConvertOpts {
        ConvertOpts {
            input: None,
            output: None,
            no_y: false,
            add_dimension_to_output: false,
            add_from_dimension: false,
            output_format: OutputFormat::Space,
            mc_version: McVersion::New,
            ignore_invalid_coords: false,
        }
    }

    #[test]
    fn nether_to_overworld() {
        let c = Coords {
            x: 100,
            y: Some(64),
            z: 200,
        };
        assert_eq!(
            c.convert(World::Nether).unwrap(),
            Coords {
                x: 800,
                y: Some(64),
                z: 1600,
            }
        );
    }

    #[test]
    fn overworld_to_nether() {
        let c = Coords {
            x: 800,
            y: Some(64),
            z: 1600,
        };
        assert_eq!(
            c.convert(World::Overworld).unwrap(),
            Coords {
                x: 100,
                y: Some(64),
                z: 200,
            }
        );
    }

    #[test]
    fn overworld_to_nether_truncates_toward_zero() {
        assert_eq!(
            Coords {
                x: 100,
                y: None,
                z: 200,
            }
            .convert(World::Overworld)
            .unwrap(),
            Coords {
                x: 12,
                y: None,
                z: 25,
            }
        );
        assert_eq!(
            Coords {
                x: -100,
                y: None,
                z: -200,
            }
            .convert(World::Overworld)
            .unwrap(),
            Coords {
                x: -12,
                y: None,
                z: -25,
            }
        );
    }

    #[test]
    fn world_aliases() {
        assert_eq!(World::parse("n"), Some(World::Nether));
        assert_eq!(World::parse("nether"), Some(World::Nether));
        assert_eq!(World::parse("NETHER"), Some(World::Nether));
        assert_eq!(World::parse("o"), Some(World::Overworld));
        assert_eq!(World::parse("ow"), Some(World::Overworld));
        assert_eq!(World::parse("Overworld"), Some(World::Overworld));
        assert_eq!(World::parse("x"), None);
    }

    #[test]
    fn coords_spaced() {
        assert_eq!(
            Coords::parse(&["100", "64", "200"]).unwrap(),
            Coords {
                x: 100,
                y: Some(64),
                z: 200,
            }
        );
        assert_eq!(
            Coords::parse(&["100", "200"]).unwrap(),
            Coords {
                x: 100,
                y: None,
                z: 200,
            }
        );
    }

    #[test]
    fn coords_comma() {
        assert_eq!(
            Coords::parse(&["100,64,200"]).unwrap(),
            Coords {
                x: 100,
                y: Some(64),
                z: 200,
            }
        );
        assert_eq!(
            Coords::parse(&["100,200"]).unwrap(),
            Coords {
                x: 100,
                y: None,
                z: 200,
            }
        );
    }

    #[test]
    fn coords_invalid() {
        assert!(Coords::parse(&["100"]).is_err());
        assert!(Coords::parse(&["100", "64", "200", "1"]).is_err());
        assert!(Coords::parse(&["abc", "64", "200"]).is_err());
    }

    #[test]
    fn parse_line_with_world() {
        let item = parse_line("nether 100 64 200", None).unwrap();
        assert_eq!(item.from, World::Nether);
        assert_eq!(
            item.coords,
            Coords {
                x: 100,
                y: Some(64),
                z: 200,
            }
        );
    }

    #[test]
    fn parse_line_default_world() {
        let item = parse_line("800,800", Some(World::Overworld)).unwrap();
        assert_eq!(item.from, World::Overworld);
        assert_eq!(
            item.coords,
            Coords {
                x: 800,
                y: None,
                z: 800,
            }
        );
    }

    #[test]
    fn render_default() {
        let item = Item {
            from: World::Nether,
            coords: Coords {
                x: 100,
                y: Some(64),
                z: 200,
            },
        };
        assert_eq!(render(&item, &convert_opts()).unwrap(), "800 64 1600");
    }

    #[test]
    fn render_no_y() {
        let item = Item {
            from: World::Nether,
            coords: Coords {
                x: 100,
                y: Some(64),
                z: 100,
            },
        };
        let mut o = convert_opts();
        o.no_y = true;
        assert_eq!(render(&item, &o).unwrap(), "800 800");
    }

    #[test]
    fn render_add_dimension_to_output() {
        let item = Item {
            from: World::Overworld,
            coords: Coords {
                x: 800,
                y: None,
                z: 800,
            },
        };
        let mut o = convert_opts();
        o.add_dimension_to_output = true;
        assert_eq!(render(&item, &o).unwrap(), "nether 100 100");
    }

    #[test]
    fn render_add_from_dimension() {
        let item = Item {
            from: World::Overworld,
            coords: Coords {
                x: 800,
                y: Some(64),
                z: 800,
            },
        };
        let mut o = convert_opts();
        o.add_from_dimension = true;
        assert_eq!(
            render(&item, &o).unwrap(),
            "Overworld 800 64 800 -> Nether 100 64 100"
        );
    }

    #[test]
    fn render_comma_sep() {
        let item = Item {
            from: World::Overworld,
            coords: Coords {
                x: 800,
                y: Some(64),
                z: 100,
            },
        };
        let mut o = convert_opts();
        o.output_format = OutputFormat::Comma;
        assert_eq!(render(&item, &o).unwrap(), "100,64,12");
    }

    #[test]
    fn cli_example_comma_sep_batch() {
        let lines = ["o 800,800", "n 100 64 100"];
        let items: Vec<Item> = lines.iter().map(|l| parse_line(l, None).unwrap()).collect();
        let mut o = convert_opts();
        o.output_format = OutputFormat::Comma;
        let out: Vec<String> = items.iter().map(|item| render(item, &o).unwrap()).collect();
        assert_eq!(out, vec!["100,100", "800,64,800"]);
    }

    #[test]
    fn convert_valid_coords() {
        let opts = convert_opts();
        let items = vec![Item {
            from: World::Nether,
            coords: Coords {
                x: 100,
                y: Some(64),
                z: 200,
            },
        }];
        assert_eq!(
            render_items(&items, &opts).unwrap(),
            vec!["800 64 1600"]
        );
    }

    #[test]
    fn convert_invalid_default_errors() {
        let opts = ConvertOpts {
            mc_version: McVersion::Old,
            ..convert_opts()
        };
        let items = vec![Item {
            from: World::Nether,
            coords: Coords {
                x: 0,
                y: Some(128),
                z: 0,
            },
        }];
        let err = render_items(&items, &opts).unwrap_err();
        assert!(err.contains("invalid coords"));
    }

    #[test]
    fn convert_invalid_out_of_bounds_x_errors() {
        let opts = convert_opts();
        let items = vec![Item {
            from: World::Overworld,
            coords: Coords {
                x: 30_000_000,
                y: None,
                z: 0,
            },
        }];
        assert!(render_items(&items, &opts).is_err());
    }

    #[test]
    fn convert_invalid_ignored_ok() {
        let opts = ConvertOpts {
            mc_version: McVersion::Old,
            ignore_invalid_coords: true,
            ..convert_opts()
        };
        let items = vec![Item {
            from: World::Nether,
            coords: Coords {
                x: 0,
                y: Some(128),
                z: 0,
            },
        }];
        assert_eq!(render_items(&items, &opts).unwrap(), vec!["0 128 0"]);
    }

    #[test]
    fn convert_overflow_errors() {
        let c = Coords {
            x: i64::MAX / 4,
            y: None,
            z: 0,
        };
        assert!(c.convert(World::Nether).is_err());
    }

    #[test]
    fn mc_version_parse() {
        assert_eq!(McVersion::parse("old"), Ok(McVersion::Old));
        assert_eq!(McVersion::parse("new"), Ok(McVersion::New));
        assert!(McVersion::parse("1.18").is_err());
    }

    #[test]
    fn validate_new_limits() {
        let v = McVersion::New;
        let c = |x: i64, y: i64, z: i64| Coords { x, y: Some(y), z };
        assert!(v.is_valid(World::Overworld, &c(29_999_984, 64, 29_999_984)));
        assert!(!v.is_valid(World::Overworld, &c(30_000_000, 64, 0)));
        assert!(!v.is_valid(World::Overworld, &c(0, -65, 0)));
        assert!(v.is_valid(World::Overworld, &c(0, -64, 0)));
        assert!(!v.is_valid(World::Overworld, &c(0, 320, 0)));
        assert!(v.is_valid(World::Overworld, &c(0, 319, 0)));
    }

    #[test]
    fn validate_old_limits() {
        let v = McVersion::Old;
        let c = |x: i64, y: i64, z: i64| Coords { x, y: Some(y), z };
        assert!(v.is_valid(World::Overworld, &c(30_000_000, 255, 30_000_000)));
        assert!(!v.is_valid(World::Overworld, &c(30_000_001, 0, 0)));
        assert!(!v.is_valid(World::Overworld, &c(0, -1, 0)));
        assert!(v.is_valid(World::Overworld, &c(0, 0, 0)));
        assert!(!v.is_valid(World::Overworld, &c(0, 256, 0)));
    }

    #[test]
    fn validate_old_nether_y_ceiling() {
        let v = McVersion::Old;
        let c = |y: i64| Coords { x: 0, y: Some(y), z: 0 };
        assert!(v.is_valid(World::Nether, &c(0)));
        assert!(v.is_valid(World::Nether, &c(127)));
        assert!(!v.is_valid(World::Nether, &c(128)));
        assert!(!v.is_valid(World::Nether, &c(-1)));
    }

    #[test]
    fn validate_without_y() {
        assert!(McVersion::New.is_valid(World::Overworld, &Coords {
            x: 0,
            y: None,
            z: 29_999_984
        }));
        assert!(!McVersion::New.is_valid(World::Overworld, &Coords {
            x: 29_999_985,
            y: None,
            z: 0
        }));
    }

    #[test]
    fn parse_validate_line_with_dimension() {
        let (dim, coords) = parse_validate_line("nether 0 128 0", World::Overworld).unwrap();
        assert_eq!(dim, World::Nether);
        assert_eq!(
            coords,
            Coords {
                x: 0,
                y: Some(128),
                z: 0,
            }
        );
    }

    #[test]
    fn parse_validate_line_default_dimension() {
        let (dim, coords) = parse_validate_line("800 800", World::Nether).unwrap();
        assert_eq!(dim, World::Nether);
        assert_eq!(
            coords,
            Coords {
                x: 800,
                y: None,
                z: 800,
            }
        );
    }
}
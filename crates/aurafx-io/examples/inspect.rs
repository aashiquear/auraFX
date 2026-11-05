//! Print what a file holds at one time step: blocks, counts, bounds and array ranges.
//! `cargo run --release -p aurafx-io --example inspect -- file [step] [key=value ...]` (reader options)

use aurafx_data::DataSet;
use aurafx_io::{load_step, read_file, Loaded};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: inspect file [step]");
    let step: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let opts: aurafx_io::ReaderOptions = args.iter().skip(3).filter_map(|a| a.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for o in aurafx_io::reader_options(path) {
        println!("option {} = {} ({})", o.key, opts.get(&o.key).unwrap_or(&o.default), o.label);
    }
    let ds = aurafx_io::with_options(&opts, || -> anyhow::Result<_> {
        Ok(match read_file(path)? {
            Loaded::Static(d) => d,
            Loaded::Series(s) => {
                println!("{} steps; step {step} at t = {}", s.times.len(), s.times.get(step).copied().unwrap_or(f64::NAN));
                load_step(&s.steps[step.min(s.steps.len() - 1)])?
            }
        })
    })?;
    let leaves: Vec<(String, &DataSet)> = match &*ds {
        DataSet::MultiBlock(mb) => mb.leaves(),
        d => vec![("".into(), d)],
    };
    for (name, d) in leaves {
        let b = d.bounds();
        println!("{name}: {} points, {} cells, bounds {:?} {:?}", d.n_points(), d.n_cells(), b.min, b.max);
        for (what, attrs) in [("point", d.point_data()), ("cell", d.cell_data())] {
            for a in attrs.into_iter().flat_map(|a| a.iter()) {
                println!("  {what} {} ({}): {:?}", a.name, a.ncomp, a.range(None));
            }
        }
    }
    Ok(())
}

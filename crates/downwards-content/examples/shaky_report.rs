//! Summarise shaky-hand robustness for every authored dungeon route.
//!
//! Parses the frozen witness artifact (no re-solving) and reports, per route,
//! the strength-one survival rate of each noise family plus the worst-family
//! rate, then prints the distribution across the dungeon and flags outliers.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example shaky_report`

const WITNESS_ARTIFACT: &str = include_str!("../generated/demo-dungeon-witnesses-v1.txt");

/// Routes whose worst strength-one family survives less often than this are
/// flagged as fragile. Survival rates are evidence about the recorded witness
/// schedule, not a human-difficulty score.
const FRAGILE_SURVIVAL_PERCENT: usize = 75;

#[derive(Debug)]
struct RouteRobustness {
    route: String,
    /// (family name, successes, trials) at strength one.
    families: Vec<(String, usize, usize)>,
}

impl RouteRobustness {
    fn worst_percent(&self) -> usize {
        self.families
            .iter()
            .map(|(_, successes, trials)| successes * 100 / trials)
            .min()
            .unwrap_or(100)
    }

    fn mean_percent(&self) -> usize {
        if self.families.is_empty() {
            return 100;
        }
        self.families
            .iter()
            .map(|(_, successes, trials)| successes * 100 / trials)
            .sum::<usize>()
            / self.families.len()
    }
}

fn parse_routes(artifact: &str) -> Vec<RouteRobustness> {
    let mut routes = Vec::new();
    let mut current: Option<RouteRobustness> = None;
    for line in artifact.lines() {
        if let Some(route) = line.strip_prefix("route ") {
            current = Some(RouteRobustness {
                route: route.to_owned(),
                families: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix("shaky ") {
            let mut fields = rest.split_whitespace();
            let family = fields.next().expect("shaky family").to_owned();
            let successes = fields
                .next()
                .and_then(|field| field.parse().ok())
                .expect("shaky successes");
            let trials = fields
                .next()
                .and_then(|field| field.parse().ok())
                .expect("shaky trials");
            assert!(trials > 0, "shaky line with zero trials");
            current
                .as_mut()
                .expect("shaky line outside a route")
                .families
                .push((family, successes, trials));
        } else if line == "end" {
            routes.push(current.take().expect("end without a route"));
        }
    }
    assert!(current.is_none(), "unterminated route record");
    routes
}

fn main() {
    let mut routes = parse_routes(WITNESS_ARTIFACT);
    assert!(!routes.is_empty(), "no routes in witness artifact");
    routes.sort_by_key(RouteRobustness::worst_percent);

    println!("strength-one shaky survival by route (worst family first):");
    println!("{:>3} {:<40} {:>5} {:>5}  families", "#", "route", "worst", "mean");
    for (index, route) in routes.iter().enumerate() {
        let families = route
            .families
            .iter()
            .map(|(family, successes, trials)| format!("{family} {successes}/{trials}"))
            .collect::<Vec<_>>()
            .join("  ");
        let flag = if route.worst_percent() < FRAGILE_SURVIVAL_PERCENT {
            "  FRAGILE"
        } else {
            ""
        };
        println!(
            "{:>3} {:<40} {:>4}% {:>4}%  {}{}",
            index + 1,
            route.route,
            route.worst_percent(),
            route.mean_percent(),
            families,
            flag,
        );
    }

    let mut histogram = [0usize; 11];
    for route in &routes {
        histogram[route.worst_percent() / 10] += 1;
    }
    println!("\ndistribution of worst-family survival ({} routes):", routes.len());
    for (bucket, count) in histogram.iter().enumerate() {
        let label = if bucket == 10 {
            "  100%".to_owned()
        } else {
            format!("{:>2}-{:>2}%", bucket * 10, bucket * 10 + 9)
        };
        println!("{label} {:>3} {}", count, "#".repeat(*count));
    }

    let fragile = routes
        .iter()
        .filter(|route| route.worst_percent() < FRAGILE_SURVIVAL_PERCENT)
        .count();
    println!(
        "\n{fragile} of {} routes fall below {FRAGILE_SURVIVAL_PERCENT}% worst-family survival",
        routes.len()
    );
}

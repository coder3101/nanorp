//! Shared avatar helpers: fallback initial and a deterministic gradient so a
//! character gets the same colors everywhere it appears.

/// First character of a name, uppercased, for fallback avatars.
pub fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Deterministic gradient classes derived from a name.
pub fn gradient(name: &str) -> &'static str {
    const GRADIENTS: [&str; 8] = [
        "from-rose-500 to-orange-500",
        "from-sky-500 to-indigo-500",
        "from-emerald-500 to-teal-500",
        "from-violet-500 to-fuchsia-500",
        "from-amber-500 to-pink-500",
        "from-cyan-500 to-blue-500",
        "from-lime-500 to-emerald-500",
        "from-fuchsia-500 to-purple-600",
    ];
    let sum: u32 = name.bytes().map(|b| b as u32).sum();
    GRADIENTS[(sum as usize) % GRADIENTS.len()]
}

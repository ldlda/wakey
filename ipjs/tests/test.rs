use std::collections::HashSet;

use lda_ipjs::subcommands::neighbor;

#[tokio::test] // ← Use tokio::test instead of manual #[tokio::main]
async fn ball1() -> anyhow::Result<()> {
    let result = neighbor::nl::get(None, None, &[]).await?;
    println!("netlink results: {:?}", result);
    Ok(()) // ← Don't force error, let it succeed
}

#[tokio::test]
async fn ball2() -> anyhow::Result<()> {
    let result = neighbor::json::get(None, None, &[]).await?;
    println!("json results: {:?}", result);
    Ok(())
}

// Add this to debug the raw JSON
#[tokio::test]
async fn ball_raw_json() -> anyhow::Result<()> {
    let output = tokio::process::Command::new("ip")
        .args(["-j", "neigh", "show"])
        .output()
        .await?;

    let json = String::from_utf8_lossy(&output.stdout);
    println!("Raw JSON:\n{}", json);

    // Try to parse it
    let parsed: Result<Vec<neighbor::NeighborItem>, _> = serde_json::from_slice(&output.stdout);
    match parsed {
        Ok(items) => println!("Parsed {} items", items.len()),
        Err(e) => println!("Parse error: {}", e),
    }

    Ok(())
}

// Check what fields actually exist in the JSON
#[tokio::test]
async fn ball_field_analysis() -> anyhow::Result<()> {
    let output = tokio::process::Command::new("ip")
        .args(["-j", "neigh", "show"])
        .output()
        .await?;

    let raw: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)?;

    println!("Found {} neighbor entries", raw.len());

    // Collect all unique field names across all entries
    let mut all_fields = std::collections::HashSet::new();
    for (i, entry) in raw.iter().enumerate() {
        if let Some(obj) = entry.as_object() {
            println!("\nEntry {}: {} fields", i, obj.len());
            for (key, value) in obj {
                all_fields.insert(key.clone());
                println!("  {}: {} = {:?}", key, value.type_name(), value);
            }
        }
    }

    println!("\n=== All unique fields seen ===");
    for field in &all_fields {
        println!("  - {}", field);
    }

    Ok(())
}

// Helper trait to get type name for JSON values
trait TypeName {
    fn type_name(&self) -> &str;
}

impl TypeName for serde_json::Value {
    fn type_name(&self) -> &str {
        match self {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "bool",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        }
    }
}

// Test filtering logic
#[tokio::test]
async fn ball_compare_backends() -> anyhow::Result<()> {
    println!("=== JSON Backend ===");
    let json_result = neighbor::json::get(None, None, &[]).await?;
    println!("Got {} entries from JSON", json_result.len());

    println!("\n=== Netlink Backend ===");
    let nl_result = neighbor::nl::get(None, None, &[]).await?;
    println!("Got {} entries from netlink", nl_result.len());

    // Compare counts
    if json_result.len() != nl_result.len() {
        println!(
            "\n⚠️ Count mismatch! JSON: {}, Netlink: {}",
            json_result.len(),
            nl_result.len()
        );
    } else {
        println!("\n✅ Both backends returned same count");
    }
    let a: HashSet<neighbor::NeighborItem> = HashSet::from_iter(json_result);
    let b: HashSet<neighbor::NeighborItem> = HashSet::from_iter(nl_result);
    println!(
        "istg {len1} == {len2} or else",
        len1 = a.len(),
        len2 = b.len()
    );

    Ok(())
}

#[tokio::test]
async fn cidr_filter() {
    unimplemented!("never. i aint add what i dont need")
}

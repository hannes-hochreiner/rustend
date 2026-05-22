use sqlx::{PgPool, Row};
use rustend_core::{
    ClientId, CreatedAtFilter, FilterCondition, FilterOperator,
    HeadAction, ObjectId, ObjectUpdate, Revision, TransactionId,
};
use uuid::Uuid;
use crate::db::revisions::{get_revision_rows_by_ids, get_parents_batch, row_to_revision_sync};

pub async fn fetch_object_updates(
    pool: &PgPool,
    client_id: ClientId,
    since: Option<TransactionId>,
    up_to: TransactionId,
    object_types: Option<&[String]>,
    created_at_filters: Option<&[CreatedAtFilter]>,
    content_filter: Option<&Vec<Vec<FilterCondition>>>,
) -> Result<Vec<ObjectUpdate>, sqlx::Error> {
    let since_id = since.map(|t| t.0 as i64).unwrap_or(0);
    let up_to_id: i64 = up_to.0.try_into().map_err(|_| sqlx::Error::Protocol(
        "up_to transaction ID out of range".into(),
    ))?;

    let changed_rows = sqlx::query(
        r#"
        SELECT DISTINCT r.object_id
        FROM revisions r
        JOIN transaction_revisions tr ON tr.revision_id = r.id
        JOIN transactions t ON t.id = tr.transaction_id
        WHERE t.id > $1
          AND t.id <= $2
          AND r.created_by != $3
        "#
    )
    .bind(since_id)
    .bind(up_to_id)
    .bind(client_id.0)
    .fetch_all(pool)
    .await?;

    let changed_objects: Vec<Uuid> = changed_rows
        .into_iter()
        .map(|r| r.get("object_id"))
        .collect();

    if changed_objects.is_empty() {
        return Ok(vec![]);
    }

    let head_rows = sqlx::query(
        "SELECT object_id, revision_id FROM object_heads WHERE object_id = ANY($1)"
    )
    .bind(&changed_objects)
    .fetch_all(pool)
    .await?;

    let mut heads_by_object: std::collections::HashMap<Uuid, Vec<Uuid>> =
        std::collections::HashMap::new();
    for row in &head_rows {
        let oid: Uuid = row.get("object_id");
        let rid: Uuid = row.get("revision_id");
        heads_by_object.entry(oid).or_default().push(rid);
    }

    let all_head_ids: Vec<Uuid> = head_rows.iter().map(|r| r.get("revision_id")).collect();
    let revision_rows = get_revision_rows_by_ids(pool, &all_head_ids).await?;
    let mut rows_by_id: std::collections::HashMap<Uuid, _> =
        revision_rows.into_iter().map(|r| (r.id, r)).collect();
    let parents_map = get_parents_batch(pool, &all_head_ids).await?;

    let mut updates = Vec::new();
    for object_id in &changed_objects {
        let head_ids = match heads_by_object.get(object_id) {
            Some(h) => h,
            None => continue,
        };

        let mut head_revisions: Vec<Revision> = Vec::new();
        let mut passes_filter = false;

        for head_id in head_ids {
            let row = match rows_by_id.remove(head_id) {
                Some(r) => r,
                None => continue,
            };

            // Check all filter criteria for this head
            let head_passes = match object_types {
                Some(types) if !types.contains(&row.object_type) => false,
                _ => true,
            } && match created_at_filters {
                Some(filters) => apply_created_at_filters(row.created_at, filters),
                None => true,
            } && match (&row.data, content_filter) {
                (Some(data), Some(f)) => apply_content_filter(data, f),
                (None, Some(_)) => true,
                _ => true,
            };

            if head_passes {
                passes_filter = true;
            }

            // Always collect this head — if any head matches, all heads are
            // returned so conflicts remain visible.
            let parents = parents_map.get(&row.id).cloned().unwrap_or_default();
            let revision = row_to_revision_sync(row, parents);
            head_revisions.push(revision);
        }

        if !passes_filter || head_revisions.is_empty() {
            continue;
        }

        let action = if head_revisions.len() == 1 {
            HeadAction::Replace
        } else {
            HeadAction::Conflict
        };

        updates.push(ObjectUpdate {
            object_id: ObjectId(*object_id),
            action,
            heads: head_revisions,
        });
    }

    Ok(updates)
}

fn apply_created_at_filters(
    created_at: chrono::DateTime<chrono::Utc>,
    filters: &[CreatedAtFilter],
) -> bool {
    filters.iter().all(|f| match f {
        CreatedAtFilter::Gt(t)  => &created_at > t,
        CreatedAtFilter::Gte(t) => &created_at >= t,
        CreatedAtFilter::Lt(t)  => &created_at < t,
        CreatedAtFilter::Lte(t) => &created_at <= t,
    })
}

fn apply_content_filter(
    data: &serde_json::Value,
    filter: &[Vec<FilterCondition>],
) -> bool {
    filter.iter().any(|and_group| {
        and_group.iter().all(|cond| evaluate_condition(data, cond))
    })
}

fn evaluate_condition(data: &serde_json::Value, cond: &FilterCondition) -> bool {
    let value = resolve_path(data, &cond.path);
    match &cond.operator {
        FilterOperator::Exists        => value.is_some(),
        FilterOperator::IsNull        => value.map(|v| v.is_null()).unwrap_or(false),
        FilterOperator::Eq(v)         => value.map(|d| d == v).unwrap_or(false),
        FilterOperator::Ne(v)         => value.map(|d| d != v).unwrap_or(true),
        FilterOperator::Gt(v)         => value.and_then(|d| compare(d, v)).map(|o| o > 0).unwrap_or(false),
        FilterOperator::Gte(v)        => value.and_then(|d| compare(d, v)).map(|o| o >= 0).unwrap_or(false),
        FilterOperator::Lt(v)         => value.and_then(|d| compare(d, v)).map(|o| o < 0).unwrap_or(false),
        FilterOperator::Lte(v)        => value.and_then(|d| compare(d, v)).map(|o| o <= 0).unwrap_or(false),
        FilterOperator::Contains(v)   => value.map(|d| json_contains(d, v)).unwrap_or(false),
        FilterOperator::StartsWith(s) => value
            .and_then(|d| d.as_str())
            .map(|s2| s2.starts_with(s.as_str()))
            .unwrap_or(false),
    }
}

fn resolve_path<'a>(data: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let path = path.strip_prefix("$.").unwrap_or(path);
    let mut current = data;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}

fn compare(a: &serde_json::Value, b: &serde_json::Value) -> Option<i32> {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
            let fx = x.as_f64()?;
            let fy = y.as_f64()?;
            Some(fx.partial_cmp(&fy).map(|o| match o {
                std::cmp::Ordering::Less    => -1,
                std::cmp::Ordering::Equal   =>  0,
                std::cmp::Ordering::Greater =>  1,
            }).unwrap_or(0))
        }
        (serde_json::Value::String(x), serde_json::Value::String(y)) =>
            Some(x.as_str().cmp(y.as_str()) as i32),
        _ => None,
    }
}

fn json_contains(data: &serde_json::Value, needle: &serde_json::Value) -> bool {
    match data {
        serde_json::Value::Array(arr) => arr.contains(needle),
        serde_json::Value::String(s) =>
            needle.as_str().map(|n| s.contains(n)).unwrap_or(false),
        _ => false,
    }
}

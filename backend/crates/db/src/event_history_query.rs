use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::Result;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct EventCursorPosition {
    pub created_at: DateTime<Utc>,
    pub id: i64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UuidFilter {
    column: &'static str,
    value: Uuid,
}

impl UuidFilter {
    pub(crate) fn new(column: &'static str, value: Uuid) -> Self {
        Self { column, value }
    }
}

pub(crate) async fn list_event_rows<R>(
    pool: &PgPool,
    table: &'static str,
    columns: &'static str,
    required: UuidFilter,
    optional: Option<UuidFilter>,
    limit: i64,
    cursor: Option<EventCursorPosition>,
) -> Result<Vec<R>>
where
    for<'row> R: FromRow<'row, PgRow> + Send + Unpin,
{
    let mut next_param = 2;
    let mut filters = format!("{} = $1", required.column);
    if let Some(optional) = optional {
        filters.push_str(&format!(" AND {} = ${next_param}", optional.column));
        next_param += 1;
    }
    if cursor.is_some() {
        let created_at_param = next_param;
        let id_param = next_param + 1;
        filters.push_str(&format!(
            " AND (created_at, id) < (${created_at_param}, ${id_param})"
        ));
        next_param += 2;
    }

    let sql = format!(
        "SELECT {columns} FROM {table} \
         WHERE {filters} \
         ORDER BY created_at DESC, id DESC LIMIT ${next_param}"
    );
    let mut query = sqlx::query_as::<_, R>(&sql).bind(required.value);
    if let Some(optional) = optional {
        query = query.bind(optional.value);
    }
    if let Some(cursor) = cursor {
        query = query.bind(cursor.created_at).bind(cursor.id);
    }
    let rows = query
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_error)?;

    Ok(rows)
}

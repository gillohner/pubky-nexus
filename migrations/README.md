# Event Date Filtering - Quick Setup Guide

## Problem
The date range filtering won't work until existing events have the `dtstart_timestamp` field populated.

## Quick Fix - Run Migration

### Option 1: Using Neo4j Browser (Recommended)
1. Open Neo4j Browser (usually at http://localhost:7474)
2. Copy and paste the contents of `migrations/event_dtstart_timestamp.cypher`
3. Run each query step by step
4. Verify the last query shows events with timestamps

### Option 2: Using cypher-shell
```bash
cd /path/to/pubky-nexus
cat migrations/event_dtstart_timestamp.cypher | docker exec -i neo4j cypher-shell -u neo4j -p your-password
```

### Option 3: Reindex Events (Longer but more thorough)
```bash
cd /path/to/pubky-nexus
cargo run --bin nexusd -- reindex events
```

## Verify It Works

After running the migration, check in Neo4j Browser:
```cypher
MATCH (e:Event)
WHERE e.dtstart_timestamp IS NOT NULL
RETURN e.dtstart, e.dtstart_timestamp, e.summary
ORDER BY e.dtstart_timestamp ASC
LIMIT 5;
```

You should see events with populated `dtstart_timestamp` values.

## Expected Result

- **Before**: Date filtering returns no results (dtstart_timestamp is NULL)
- **After**: Date filtering works correctly, shows events in chronological order

## Performance Optimization

After migration, add an index for better query performance:

```cypher
CREATE INDEX event_dtstart_timestamp IF NOT EXISTS
FOR (e:Event)
ON (e.dtstart_timestamp);
```

## Frontend Changes

The frontend now:
- ✅ Defaults to showing events from today onwards
- ✅ Provides date picker inputs for start/end dates
- ✅ Shows clear date range badge
- ✅ Syncs date range to URL for shareable links

## Troubleshooting

### Date filtering still not working?
1. Check the backend logs to see if `start_date` parameter is received
2. Verify events have `dtstart_timestamp` set (see "Verify It Works" above)
3. Make sure you're using microseconds (not milliseconds) for timestamps

### Events not in chronological order?
This means `dtstart_timestamp` is NULL for those events. Run the migration again.

### Frontend shows "All events" when it should show "From [date]"?
Check browser console - there might be a date parsing issue. Timestamps should be in microseconds.

// Migration: Add dtstart_timestamp to existing Event nodes
// This parses the ISO 8601 dtstart string into Unix microseconds timestamp

// Step 1: Check how many events need migration
MATCH (e:Event)
WHERE e.dtstart_timestamp IS NULL AND e.dtstart IS NOT NULL
RETURN count(e) as events_to_migrate;

// Step 2: Update events with dtstart_timestamp
// This handles multiple ISO 8601 formats
MATCH (e:Event)
WHERE e.dtstart_timestamp IS NULL AND e.dtstart IS NOT NULL
WITH e,
  CASE
    // ISO8601 with timezone offset (e.g., "2025-12-27T02:49:02+00:00" or "2025-12-27T02:49:02Z")
    WHEN e.dtstart =~ '\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}.*[Zz+-].*'
      THEN datetime(e.dtstart)
    
    // ISO8601 without timezone (assume UTC) (e.g., "2025-12-27T02:49:02")
    WHEN e.dtstart =~ '\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}'
      THEN datetime(e.dtstart + 'Z')
    
    // Date only, no time (e.g., "2025-12-27")
    WHEN e.dtstart =~ '\\d{4}-\\d{2}-\\d{2}'
      THEN datetime(e.dtstart + 'T00:00:00Z')
    
    ELSE NULL
  END AS parsed_datetime

WHERE parsed_datetime IS NOT NULL
// Convert to Unix microseconds: epochSeconds * 1000000 + milliseconds * 1000
SET e.dtstart_timestamp = (parsed_datetime.epochSeconds * 1000000) + (parsed_datetime.epochMillis % 1000 * 1000)

RETURN count(e) as updated_events;

// Step 3: Verify the migration
MATCH (e:Event)
RETURN 
  count(e) as total_events,
  count(e.dtstart_timestamp) as events_with_timestamp,
  count(e) - count(e.dtstart_timestamp) as events_without_timestamp;

// Step 4: Sample some results to verify correctness
MATCH (e:Event)
WHERE e.dtstart_timestamp IS NOT NULL
RETURN e.dtstart, e.dtstart_timestamp
ORDER BY e.dtstart_timestamp ASC
LIMIT 10;

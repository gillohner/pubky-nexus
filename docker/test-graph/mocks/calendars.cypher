// Calendar Mock Data for Pubky Nexus
// Based on v2-explicit-fields iCalendar integration

// Set up user parameters (using existing users from posts.cypher)
:param amsterdam => 'emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy';

:param bogota => 'ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny';

:param cairo => 'f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o';

:param detroit => '7w4hmktqa7gia5thmk7zki8px7ttwpwjtgaaaou4tbqx64re8d1o';

:param eixample => '8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy';

// Add some additional users for testing
:param satoshi => 'satoshi1234567890abcdefghijklmnopqrstuvwxyz123';

:param hal => 'halfinn1234567890abcdefghijklmnopqrstuvwxyz123';

:param adamback => 'adamback1234567890abcdefghijklmnopqrstuvwxyz12';

// ##############################
// ##### Create additional users
// ##############################
MERGE (u:User { id: $satoshi }) SET u.name = "Satoshi Nakamoto", u.bio = "Bitcoin creator", u.status = "active", u.indexed_at = 1698753600000, u.links = "[]";

MERGE (u:User { id: $hal }) SET u.name = "Hal Finney", u.bio = "Cryptographer", u.status = "active", u.indexed_at = 1698753600000, u.links = "[]";

MERGE (u:User { id: $adamback }) SET u.name = "Adam Back", u.bio = "Hashcash inventor", u.status = "active", u.indexed_at = 1698753600000, u.links = "[]";

// ##############################
// ##### Create Calendars #######
// ##############################

// Calendar 1: Bitcoin Switzerland Events (by Satoshi, admins: adamback, amsterdam)
MERGE (c1:Calendar { id: "0033RCZXVEPNG" })
 SET c1.uri = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/calendar/0033RCZXVEPNG",
c1.name = "Bitcoin Switzerland Events",
c1.image_uri = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/files/0033CALIMG01",
c1.color = "#F7931A",
c1.timezone = "Europe/Zurich",
c1.created = 1698753600000,
c1.indexed_at = 1698753600000;

MATCH (author:User { id: $satoshi }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (author)-[:AUTHORED]->(cal);

// Create ADMIN relationships
MATCH (admin:User { id: $adamback }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (admin)-[:ADMIN { indexed_at: 1698753600000 }]->(cal);

MATCH (admin:User { id: $amsterdam }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (admin)-[:ADMIN { indexed_at: 1698753600000 }]->(cal);

// Calendar 2: Lightning Network Meetups (by Hal, admin: detroit)
MERGE (c2:Calendar { id: "0033RDZXVEPNG" })
 SET c2.uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/calendar/0033RDZXVEPNG",
c2.name = "Lightning Network Meetups",
c2.image_uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/files/0033CALIMG02",
c2.color = "#FFD700",
c2.timezone = "Europe/Zurich",
c2.created = 1698753600000,
c2.indexed_at = 1698753600000;

MATCH (author:User { id: $hal }), (cal:Calendar {id: "0033RDZXVEPNG"})
MERGE (author)-[:AUTHORED]->(cal);

MATCH (admin:User { id: $detroit }), (cal:Calendar {id: "0033RDZXVEPNG"})
MERGE (admin)-[:ADMIN { indexed_at: 1698753600000 }]->(cal);

// Calendar 3: Pubky Developer Events (by Amsterdam, no additional admins)
MERGE (c3:Calendar { id: "0033REZXVEPNG" })
 SET c3.uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/calendar/0033REZXVEPNG",
c3.name = "Pubky Developer Events",
c3.image_uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/files/0033CALIMG03",
c3.color = "#9333EA",
c3.timezone = "America/New_York",
c3.created = 1699000000000,
c3.indexed_at = 1699000000000;

MATCH (author:User { id: $amsterdam }), (cal:Calendar {id: "0033REZXVEPNG"})
MERGE (author)-[:AUTHORED]->(cal);

// Calendar 4: Crypto Community Calendar (by Bogota, admins: cairo, eixample)
MERGE (c4:Calendar { id: "0033RFZXVEPNG" })
 SET c4.uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/calendar/0033RFZXVEPNG",
c4.name = "Crypto Community Calendar",
c4.image_uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/files/0033CALIMG04",
c4.color = "#10B981",
c4.timezone = "America/Bogota",
c4.created = 1699100000000,
c4.indexed_at = 1699100000000;

MATCH (author:User { id: $bogota }), (cal:Calendar {id: "0033RFZXVEPNG"})
MERGE (author)-[:AUTHORED]->(cal);

MATCH (admin:User { id: $cairo }), (cal:Calendar {id: "0033RFZXVEPNG"})
MERGE (admin)-[:ADMIN { indexed_at: 1699100000000 }]->(cal);

MATCH (admin:User { id: $eixample }), (cal:Calendar {id: "0033RFZXVEPNG"})
MERGE (admin)-[:ADMIN { indexed_at: 1699100000000 }]->(cal);

// ##############################
// ##### Create Events ##########
// ##############################

// Event 1: Weekly Bitcoin Meetup Zürich (recurring, on calendar 1)
MERGE (e1:Event { id: "0033SCZXVEPNG" })
 SET e1.uri = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/event/0033SCZXVEPNG",
e1.uid = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/event/0033SCZXVEPNG",
e1.dtstamp = 1698753600000,
e1.dtstart = "2024-10-31T19:00:00+01:00",
e1.dtend = "2024-10-31T22:00:00+01:00",
e1.summary = "Bitcoin Meetup Zürich",
e1.status = "CONFIRMED",
e1.organizer = "{\"uri\":\"pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123\"}",
e1.categories = ["bitcoin", "meetup", "networking"],
e1.created = 1698753600000,
e1.indexed_at = 1698753600000,
e1.rrule = "FREQ=WEEKLY;BYDAY=TH",
e1.rdate = null ,
e1.exdate = null ,
e1.recurrence_id = null ,
e1.image_uri = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/files/0033EVENT01",
e1.conference = ["{\"uri\":\"https: //meet.jit.si/bitcoin-zurich\",\"label\":\"Jitsi Meeting\"}"],
e1.location = "Insider Bar, Zürich",
e1.geo = "{\"lat\":47.366667,\"lon\":8.550000}",
e1.structured_locations = "[{\"name\":\"Insider Bar\", \"location_type\":\"ARRIVAL\", \"address\":\"Münstergasse 20, 8001 Zürich\", \"uri\":\"https: //www.openstreetmap.org/node/123456789\",\"description\":\"Main venue in Zürich city center\"}]",
e1.styled_description = "{\"fmttype\":\"text/html\",\"value\":\"<p>Weekly Bitcoin meetup discussing <strong>Lightning Network</strong> developments and Bitcoin adoption in Switzerland.</p>\"}",
e1.x_pubky_rsvp_access = "open";

MATCH (author:User { id: $satoshi }), (event:Event {id: "0033SCZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

MATCH (event:Event { id: "0033SCZXVEPNG" }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1698753600000 }]->(cal);

// Event 2: Lightning Network Workshop (single event, on calendar 2)
MERGE (e2:Event { id: "0033SDZXVEPNG" })
 SET e2.uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/event/0033SDZXVEPNG",
e2.uid = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/event/0033SDZXVEPNG",
e2.dtstamp = 1698753600000,
e2.dtstart = "2024-11-05T14:00:00+01:00",
e2.dtend = "2024-11-05T18:00:00+01:00",
e2.summary = "Lightning Network Workshop",
e2.status = "CONFIRMED",
e2.organizer = "{\"uri\":\"pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123\"}",
e2.categories = ["lightning", "workshop", "education"],
e2.created = 1698753600000,
e2.indexed_at = 1698753600000,
e2.rrule = null ,
e2.rdate = null ,
e2.exdate = null ,
e2.recurrence_id = null ,
e2.image_uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/files/0033EVENT02",
e2.conference = ["{\"uri\":\"https: //meet.jit.si/lightning-workshop\",\"label\":\"Lightning Workshop Room\"}"],
e2.location = "Tech Hub Zürich",
e2.geo = "{\"lat\":47.370000,\"lon\":8.545000}",
e2.structured_locations = "[{\"name\":\"Tech Hub Zürich\", \"location_type\":\"ARRIVAL\", \"address\":\"Technoparkstrasse 1, 8005 Zürich\", \"uri\":\"https: //www.openstreetmap.org/node/987654321\",\"description\":\"Workshop room with presentation equipment\"}]",
e2.styled_description = "{\"fmttype\":\"text/html\",\"value\":\"<p>Hands-on Lightning Network workshop covering node setup, payment channels, and best practices.</p>\"}",
e2.x_pubky_rsvp_access = "open";

MATCH (author:User { id: $hal }), (event:Event {id: "0033SDZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

MATCH (event:Event { id: "0033SDZXVEPNG" }), (cal:Calendar {id: "0033RDZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1698753600000 }]->(cal);

// Event 3: Bitcoin Conference 2024 (on calendar 1, created by admin adamback)
MERGE (e3:Event { id: "0033SEZXVEPNG" })
 SET e3.uri = "pubky: //adamback1234567890abcdefghijklmnopqrstuvwxyz12/pub/pubky.app/event/0033SEZXVEPNG",
e3.uid = "pubky: //adamback1234567890abcdefghijklmnopqrstuvwxyz12/pub/pubky.app/event/0033SEZXVEPNG",
e3.dtstamp = 1698753600000,
e3.dtstart = "2025-01-15T09:00:00+01:00",
e3.dtend = "2025-01-16T18:00:00+01:00",
e3.summary = "Bitcoin Conference 2025",
e3.status = "CONFIRMED",
e3.organizer = "{\"uri\":\"pubky: //adamback1234567890abcdefghijklmnopqrstuvwxyz12\"}",
e3.categories = ["bitcoin", "conference", "speakers"],
e3.created = 1698753600000,
e3.indexed_at = 1698753600000,
e3.rrule = null ,
e3.rdate = null ,
e3.exdate = null ,
e3.recurrence_id = null ,
e3.image_uri = "pubky: //adamback1234567890abcdefghijklmnopqrstuvwxyz12/pub/pubky.app/files/0033EVENT03",
e3.conference = ["{\"uri\":\"https: //bitcoin-conference-2025.ch\",\"label\":\"Conference Website\"}"],
e3.location = "Kongresshaus Zürich",
e3.geo = "{\"lat\":47.366667,\"lon\":8.543333}",
e3.structured_locations = "[{\"name\":\"Kongresshaus Zürich\", \"location_type\":\"ARRIVAL\", \"address\":\"Claridenstrasse 5, 8002 Zürich\", \"uri\":\"https: //www.openstreetmap.org/node/456789123\",\"description\":\"Main conference venue\"}]",
e3.styled_description = "{\"fmttype\":\"text/html\",\"value\":\"<p>The premier Bitcoin conference in Switzerland featuring keynote speakers and technical presentations.</p>\"}",
e3.x_pubky_rsvp_access = "admin";

MATCH (author:User { id: $adamback }), (event:Event {id: "0033SEZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

MATCH (event:Event { id: "0033SEZXVEPNG" }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1698753600000 }]->(cal);

// Event 4: Pubky Hackathon (on calendar 3, by Amsterdam)
MERGE (e4:Event { id: "0033SFZXVEPNG" })
 SET e4.uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/event/0033SFZXVEPNG",
e4.uid = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/event/0033SFZXVEPNG",
e4.dtstamp = 1699000000000,
e4.dtstart = "2024-11-15T10:00:00-05:00",
e4.dtend = "2024-11-17T18:00:00-05:00",
e4.summary = "Pubky Hackathon 2024",
e4.status = "CONFIRMED",
e4.organizer = "{\"uri\":\"pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy\"}",
e4.categories = ["pubky", "hackathon", "development"],
e4.created = 1699000000000,
e4.indexed_at = 1699000000000,
e4.rrule = null ,
e4.rdate = null ,
e4.exdate = null ,
e4.recurrence_id = null ,
e4.image_uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/files/0033EVENT04",
e4.conference = ["{\"uri\":\"https: //hackathon.pubky.app\",\"label\":\"Hackathon Portal\"}"],
e4.location = "NYC Tech Hub",
e4.geo = "{\"lat\":40.712776,\"lon\":-74.005974}",
e4.structured_locations = "[{\"name\":\"NYC Tech Hub\",\"location_type\":\"ARRIVAL\",\"address\":\"123 Tech Street, New York, NY 10001\",\"uri\":\"geo:40.712776,-74.005974\",\"description\":\"Main hackathon venue\"}]",
e4.styled_description = "{\"fmttype\":\"text/html\",\"value\":\"<p>Build the future of decentralized social networks with <strong>Pubky</strong>!</p>\"}",
e4.x_pubky_rsvp_access = "open";

MATCH (author:User { id: $amsterdam }), (event:Event {id: "0033SFZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

MATCH (event:Event { id: "0033SFZXVEPNG" }), (cal:Calendar {id: "0033REZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1699000000000 }]->(cal);

// Event 5: Multi-calendar event (on both calendar 1 and calendar 4)
MERGE (e5:Event { id: "0033SGZXVEPNG" })
 SET e5.uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/event/0033SGZXVEPNG",
e5.uid = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/event/0033SGZXVEPNG",
e5.dtstamp = 1699100000000,
e5.dtstart = "2024-12-01T15:00:00-05:00",
e5.dtend = "2024-12-01T17:00:00-05:00",
e5.summary = "Weekly Crypto Standup",
e5.status = "CONFIRMED",
e5.organizer = "{\"uri\":\"pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny\"}",
e5.categories = ["bitcoin", "crypto", "community"],
e5.created = 1699100000000,
e5.indexed_at = 1699100000000,
e5.rrule = null ,
e5.rdate = null ,
e5.exdate = null ,
e5.recurrence_id = null ,
e5.image_uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/files/0033EVENT05",
e5.conference = null ,
e5.location = "Community Center",
e5.geo = "{\"lat\":4.710989,\"lon\":-74.072092}",
e5.structured_locations = "[{\"name\":\"Community Center\",\"location_type\":\"ARRIVAL\",\"address\":\"Calle 45, Bogotá\",\"uri\":\"geo:4.710989,-74.072092\",\"description\":\"Community venue\"}]",
e5.styled_description = "{\"fmttype\":\"text/html\",\"value\":\"<p>Joint event bringing together Bitcoin and crypto communities.</p>\"}",
e5.x_pubky_rsvp_access = "open";

MATCH (author:User { id: $bogota }), (event:Event {id: "0033SGZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

// Link to both calendars
MATCH (event:Event { id: "0033SGZXVEPNG" }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1699100000000 }]->(cal);

MATCH (event:Event { id: "0033SGZXVEPNG" }), (cal:Calendar {id: "0033RFZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1699100000000 }]->(cal);

// Event 6: NEGATIVE TEST - Event by non-admin on calendar 1 (Cairo is not admin)
MERGE (e6:Event { id: "0033SHZXVEPNG" })
 SET e6.uri = "pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o/pub/pubky.app/event/0033SHZXVEPNG",
e6.uid = "pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o/pub/pubky.app/event/0033SHZXVEPNG",
e6.dtstamp = 1699200000000,
e6.dtstart = "2024-12-10T18:00:00+01:00",
e6.dtend = "2024-12-10T20:00:00+01:00",
e6.summary = "Unauthorized Event - Should Fail Validation",
e6.status = "TENTATIVE",
e6.organizer = "{\"uri\":\"pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o\"}",
e6.categories = ["test", "unauthorized"],
e6.created = 1699200000000,
e6.indexed_at = 1699200000000,
e6.rrule = null ,
e6.rdate = null ,
e6.exdate = null ,
e6.recurrence_id = null ,
e6.image_uri = null ,
e6.conference = null ,
e6.location = "Unknown Location",
e6.geo = null ,
e6.structured_locations = null ,
e6.styled_description = "{\"fmttype\":\"text/plain\",\"value\":\"This event should fail validation because Cairo is not an admin of the Bitcoin Switzerland Events calendar.\"}",
e6.x_pubky_rsvp_access = "open";

MATCH (author:User { id: $cairo }), (event:Event {id: "0033SHZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

// This creates an invalid relationship - Cairo is not admin of calendar 1
MATCH (event:Event { id: "0033SHZXVEPNG" }), (cal:Calendar {id: "0033RCZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1699200000000 }]->(cal);

// Event 7: Recurring event with override instance
MERGE (e7:Event { id: "0033SJZXVEPNG" })
 SET e7.uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/event/0033SJZXVEPNG",
e7.uid = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/event/0033SJZXVEPNG",
e7.dtstamp = 1699300000000,
e7.dtstart = "2024-11-07T14:00:00-05:00",
e7.dtend = "2024-11-07T16:00:00-05:00",
e7.summary = "Special Crypto Workshop",
e7.status = "CONFIRMED",
e7.organizer = "{\"uri\":\"pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny\"}",
e7.categories = ["crypto", "standup", "workshop"],
e7.created = 1731000000000,
e7.indexed_at = 1699300000000,
e7.rrule = null ,
e7.rdate = null ,
e7.exdate = null ,
e7.recurrence_id = 1731000000000,
e7.image_uri = null ,
e7.conference = ["{\"uri\":\"https: //meet.jit.si/crypto-workshop\",\"label\":\"Workshop Room\"}"],
e7.location = "Tech Hub",
e7.geo = "{\"lat\":4.710989,\"lon\":-74.072092}",
e7.structured_locations = null ,
e7.styled_description = "{\"fmttype\":\"text/html\",\"value\":\"<p>Special workshop edition with hands-on activities.</p>\"}",
e7.x_pubky_rsvp_access = "open";

MATCH (author:User { id: $bogota }), (event:Event {id: "0033SJZXVEPNG"})
MERGE (author)-[:AUTHORED]->(event);

MATCH (event:Event { id: "0033SJZXVEPNG" }), (cal:Calendar {id: "0033RFZXVEPNG"})
MERGE (event)-[:BELONGS_TO { indexed_at: 1699300000000 }]->(cal);

// ##############################
// ##### Create Attendees #######
// ##############################

// Attendee 1: Amsterdam RSVPs to all instances of Bitcoin Meetup (no recurrence_id)
MERGE (a1:Attendee { id: "0033UCZXVEPNG" })
 SET a1.uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/attendee/0033UCZXVEPNG",
a1.attendee_uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy",
a1.partstat = "ACCEPTED",
a1.role = "REQ-PARTICIPANT",
a1.recurrence_id = null ,
a1.delegated_to = null ,
a1.delegated_from = null ,
a1.created = 1698753700000,
a1.indexed_at = 1698753700000;

MATCH (author:User { id: $amsterdam }), (attendee:Attendee {id: "0033UCZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UCZXVEPNG" }), (event:Event {id: "0033SCZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1698753700000 }]->(event);

// Attendee 2: Bogota RSVPs to specific instance of Bitcoin Meetup
MERGE (a2:Attendee { id: "0033UDZXVEPNG" })
 SET a2.uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/attendee/0033UDZXVEPNG",
a2.attendee_uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny",
a2.partstat = "ACCEPTED",
a2.role = "REQ-PARTICIPANT",
a2.recurrence_id = 1699358400000,
a2.delegated_to = null ,
a2.delegated_from = null ,
a2.created = 1698753800000,
a2.indexed_at = 1698753800000;

MATCH (author:User { id: $bogota }), (attendee:Attendee {id: "0033UDZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UDZXVEPNG" }), (event:Event {id: "0033SCZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1698753800000 }]->(event);

// Attendee 3: Detroit accepts one instance and declines another
// Accept October 31
MERGE (a3a:Attendee { id: "0033UEZXVEPNG" })
 SET a3a.uri = "pubky: //7w4hmktqa7gia5thmk7zki8px7ttwpwjtgaaaou4tbqx64re8d1o/pub/pubky.app/attendee/0033UEZXVEPNG",
a3a.attendee_uri = "pubky: //7w4hmktqa7gia5thmk7zki8px7ttwpwjtgaaaou4tbqx64re8d1o",
a3a.partstat = "ACCEPTED",
a3a.role = "REQ-PARTICIPANT",
a3a.recurrence_id = 1698753600000,
a3a.delegated_to = null ,
a3a.delegated_from = null ,
a3a.created = 1698753900000,
a3a.indexed_at = 1698753900000;

MATCH (author:User { id: $detroit }), (attendee:Attendee {id: "0033UEZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UEZXVEPNG" }), (event:Event {id: "0033SCZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1698753900000 }]->(event);

// Decline November 7
MERGE (a3b:Attendee { id: "0033UFZXVEPNG" })
 SET a3b.uri = "pubky: //7w4hmktqa7gia5thmk7zki8px7ttwpwjtgaaaou4tbqx64re8d1o/pub/pubky.app/attendee/0033UFZXVEPNG",
a3b.attendee_uri = "pubky: //7w4hmktqa7gia5thmk7zki8px7ttwpwjtgaaaou4tbqx64re8d1o",
a3b.partstat = "DECLINED",
a3b.role = "REQ-PARTICIPANT",
a3b.recurrence_id = 1699358400000,
a3b.delegated_to = null ,
a3b.delegated_from = null ,
a3b.created = 1698754000000,
a3b.indexed_at = 1698754000000;

MATCH (author:User { id: $detroit }), (attendee:Attendee {id: "0033UFZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UFZXVEPNG" }), (event:Event {id: "0033SCZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1698754000000 }]->(event);

// Attendee 4: Eixample accepts Lightning Workshop
MERGE (a4:Attendee { id: "0033UGZXVEPNG" })
 SET a4.uri = "pubky: //8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy/pub/pubky.app/attendee/0033UGZXVEPNG",
a4.attendee_uri = "pubky: //8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy",
a4.partstat = "ACCEPTED",
a4.role = "REQ-PARTICIPANT",
a4.recurrence_id = null ,
a4.delegated_to = null ,
a4.delegated_from = null ,
a4.created = 1698754100000,
a4.indexed_at = 1698754100000;

MATCH (author:User { id: $eixample }), (attendee:Attendee {id: "0033UGZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UGZXVEPNG" }), (event:Event {id: "0033SDZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1698754100000 }]->(event);

// Attendee 5: Cairo tentatively accepts Bitcoin Conference
MERGE (a5:Attendee { id: "0033UHZXVEPNG" })
 SET a5.uri = "pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o/pub/pubky.app/attendee/0033UHZXVEPNG",
a5.attendee_uri = "pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o",
a5.partstat = "TENTATIVE",
a5.role = "REQ-PARTICIPANT",
a5.recurrence_id = null ,
a5.delegated_to = null ,
a5.delegated_from = null ,
a5.created = 1698754200000,
a5.indexed_at = 1698754200000;

MATCH (author:User { id: $cairo }), (attendee:Attendee {id: "0033UHZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UHZXVEPNG" }), (event:Event {id: "0033SEZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1698754200000 }]->(event);

// Attendee 6: Hal accepts Pubky Hackathon
MERGE (a6:Attendee { id: "0033UJZXVEPNG" })
 SET a6.uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/attendee/0033UJZXVEPNG",
a6.attendee_uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123",
a6.partstat = "ACCEPTED",
a6.role = "REQ-PARTICIPANT",
a6.recurrence_id = null ,
a6.delegated_to = null ,
a6.delegated_from = null ,
a6.created = 1699000100000,
a6.indexed_at = 1699000100000;

MATCH (author:User { id: $hal }), (attendee:Attendee {id: "0033UJZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UJZXVEPNG" }), (event:Event {id: "0033SFZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1699000100000 }]->(event);

// Attendee 7: Multiple people RSVP to multi-calendar event
MERGE (a7:Attendee { id: "0033UKZXVEPNG" })
 SET a7.uri = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/attendee/0033UKZXVEPNG",
a7.attendee_uri = "pubky: //satoshi1234567890abcdefghijklmnopqrstuvwxyz123",
a7.partstat = "ACCEPTED",
a7.role = "CHAIR",
a7.recurrence_id = null ,
a7.delegated_to = null ,
a7.delegated_from = null ,
a7.created = 1699100100000,
a7.indexed_at = 1699100100000;

MATCH (author:User { id: $satoshi }), (attendee:Attendee {id: "0033UKZXVEPNG"})
MERGE (author)-[:AUTHORED]->(attendee);

MATCH (attendee:Attendee { id: "0033UKZXVEPNG" }), (event:Event {id: "0033SGZXVEPNG"})
MERGE (attendee)-[:RSVP_TO { indexed_at: 1699100100000 }]->(event);

// ##############################
// ##### Create Alarms ##########
// ##############################

// Alarm 1: 15-minute reminder for Amsterdam before Bitcoin Meetup
MERGE (al1:Alarm { id: "0033WCZXVEPNG" })
 SET al1.uri = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/alarm/0033WCZXVEPNG",
al1.action = "DISPLAY",
al1.trigger = "-PT15M",
al1.description = "Bitcoin Meetup in 15 minutes",
al1.uid = "pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy/pub/pubky.app/alarm/0033WCZXVEPNG",
al1.attendees = "[\"pubky: //emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy\"]",
al1.created = 1698753700000,
al1.indexed_at = 1698753700000;

MATCH (author:User { id: $amsterdam }), (alarm:Alarm {id: "0033WCZXVEPNG"})
MERGE (author)-[:AUTHORED]->(alarm);

MATCH (alarm:Alarm { id: "0033WCZXVEPNG" }), (event:Event {id: "0033SCZXVEPNG"})
MERGE (alarm)-[:REMINDS { indexed_at: 1698753700000 }]->(event);

// Alarm 2: 1-day reminder for Eixample before Lightning Workshop
MERGE (al2:Alarm { id: "0033WDZXVEPNG" })
 SET al2.uri = "pubky: //8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy/pub/pubky.app/alarm/0033WDZXVEPNG",
al2.action = "DISPLAY",
al2.trigger = "-P1D",
al2.description = "Lightning Network Workshop tomorrow",
al2.uid = "pubky: //8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy/pub/pubky.app/alarm/0033WDZXVEPNG",
al2.attendees = "[\"pubky: //8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy\"]",
al2.created = 1698754100000,
al2.indexed_at = 1698754100000;

MATCH (author:User { id: $eixample }), (alarm:Alarm {id: "0033WDZXVEPNG"})
MERGE (author)-[:AUTHORED]->(alarm);

MATCH (alarm:Alarm { id: "0033WDZXVEPNG" }), (event:Event {id: "0033SDZXVEPNG"})
MERGE (alarm)-[:REMINDS { indexed_at: 1698754100000 }]->(event);

// Alarm 3: 1-week email reminder for Cairo before Bitcoin Conference
MERGE (al3:Alarm { id: "0033WEZXVEPNG" })
 SET al3.uri = "pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o/pub/pubky.app/alarm/0033WEZXVEPNG",
al3.action = "EMAIL",
al3.trigger = "-P7D",
al3.description = "Bitcoin Conference 2025 in one week",
al3.uid = "pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o/pub/pubky.app/alarm/0033WEZXVEPNG",
al3.attendees = "[\"pubky: //f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o\"]",
al3.created = 1698754200000,
al3.indexed_at = 1698754200000;

MATCH (author:User { id: $cairo }), (alarm:Alarm {id: "0033WEZXVEPNG"})
MERGE (author)-[:AUTHORED]->(alarm);

MATCH (alarm:Alarm { id: "0033WEZXVEPNG" }), (event:Event {id: "0033SEZXVEPNG"})
MERGE (alarm)-[:REMINDS { indexed_at: 1698754200000 }]->(event);

// Alarm 4: 2-hour reminder for Hal before Pubky Hackathon
MERGE (al4:Alarm { id: "0033WFZXVEPNG" })
 SET al4.uri = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/alarm/0033WFZXVEPNG",
al4.action = "DISPLAY",
al4.trigger = "-PT2H",
al4.description = "Pubky Hackathon starts in 2 hours",
al4.uid = "pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123/pub/pubky.app/alarm/0033WFZXVEPNG",
al4.attendees = "[\"pubky: //halfinn1234567890abcdefghijklmnopqrstuvwxyz123\"]",
al4.created = 1699000100000,
al4.indexed_at = 1699000100000;

MATCH (author:User { id: $hal }), (alarm:Alarm {id: "0033WFZXVEPNG"})
MERGE (author)-[:AUTHORED]->(alarm);

MATCH (alarm:Alarm { id: "0033WFZXVEPNG" }), (event:Event {id: "0033SFZXVEPNG"})
MERGE (alarm)-[:REMINDS { indexed_at: 1699000100000 }]->(event);

// Alarm 5: Audio reminder for Bogota for multi-calendar event
MERGE (al5:Alarm { id: "0033WGZXVEPNG" })
 SET al5.uri = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/alarm/0033WGZXVEPNG",
al5.action = "AUDIO",
al5.trigger = "-PT30M",
al5.description = "Joint meetup in 30 minutes",
al5.uid = "pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny/pub/pubky.app/alarm/0033WGZXVEPNG",
al5.attendees = "[\"pubky: //ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny\"]",
al5.created = 1699100100000,
al5.indexed_at = 1699100100000;

MATCH (author:User { id: $bogota }), (alarm:Alarm {id: "0033WGZXVEPNG"})
MERGE (author)-[:AUTHORED]->(alarm);

MATCH (alarm:Alarm { id: "0033WGZXVEPNG" }), (event:Event {id: "0033SGZXVEPNG"})
MERGE (alarm)-[:REMINDS { indexed_at: 1699100100000 }]->(event);

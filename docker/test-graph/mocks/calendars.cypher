// Set up calendar-related parameters
// Using existing users from other test files
:param amsterdam => 'emq37ky6fbnaun7q1ris6rx3mqmw3a33so1txfesg9jj3ak9ryoy';
:param bogota => 'ep441mndnsjeesenwz78r9paepm6e4kqm4ggiyy9uzpoe43eu9ny';
:param cairo => 'f5tcy5gtgzshipr6pag6cn9uski3s8tjare7wd3n7enmyokgjk1o';
:param detroit => '7w4hmktqa7gia5thmk7zki8px7ttwpwjtgaaaou4tbqx64re8d1o';
:param eixample => '8attbeo9ftu5nztqkcfw3gydksehr7jbspgfi64u4h8eo5e7dbiy';

// Calendar IDs (hash-based)
:param tech_meetup_cal => 'CAL1A2B3C4D5E6F7';
:param community_events_cal => 'CAL2G7H8I9J0K1L2';
:param workshop_series_cal => 'CAL3M3N4O5P6Q7R8';

// Event IDs (timestamp-based)
// event1: Dec 4, 2024 | event2: Dec 5, 2024 | event3: Dec 6, 2024
// event4: Dec 7, 2024 | event5: Dec 8, 2024 | event6: Dec 9, 2024
:param event1 => 'EVT1733270400000';
:param event2 => 'EVT1733356800000';
:param event3 => 'EVT1733443200000';
:param event4 => 'EVT1733529600000';
:param event5 => 'EVT1733616000000';
:param event6 => 'EVT1733702400000';

// Attendee IDs (hash-based from event URI)
:param att1_evt1 => 'ATT1A1B2C3D4E5F6';
:param att2_evt1 => 'ATT2G7H8I9J0K1L2';
:param att3_evt2 => 'ATT3M3N4O5P6Q7R8';
:param att4_evt2 => 'ATT4S9T0U1V2W3X4';
:param att5_evt3 => 'ATT5Y5Z6A7B8C9D0';
:param att6_evt4 => 'ATT6E1F2G3H4I5J6';
:param att7_evt5 => 'ATT7K7L8M9N0O1P2';
:param att8_evt6 => 'ATT8Q3R4S5T6U7V8';
:param att9_evt3 => 'ATT9W9X0Y1Z2A3B4';
:param att10_evt4 => 'ATT10C5D6E7F8G9H';
:param att11_evt5 => 'ATT11I1J2K3L4M5N';
:param att12_evt6 => 'ATT12O6P7Q8R9S0T';

// Tag labels for events
:param tech_tag => 'tech';
:param web3_tag => 'web3';
:param decentralized_tag => 'decentralized';
:param meetup_tag => 'meetup';
:param workshop_tag => 'workshop';
:param rust_tag => 'rust';
:param api_tag => 'api';
:param security_tag => 'security';
:param community_tag => 'community';
:param networking_tag => 'networking';
:param opensource_tag => 'opensource';

// ##############################
// ##### Create Calendars #######
// ##############################

// Calendar 1: Tech Meetup Calendar (owned by Amsterdam, co-admin Bogota)
MERGE (c1:Calendar {id: $tech_meetup_cal}) 
SET c1.uri = "pubky://" + $amsterdam + "/pub/eventky.app/calendars/" + $tech_meetup_cal,
    c1.name = "Tech Meetup Series",
    c1.description = "Monthly tech meetups for developers and tech enthusiasts",
    c1.timezone = "Europe/Amsterdam",
    c1.color = "#FF5733",
    c1.owner_id = $amsterdam,
    c1.created_at = 1730000000000,
    c1.indexed_at = 1730000000000;

MATCH (owner:User {id: $amsterdam}), (cal:Calendar {id: $tech_meetup_cal})
MERGE (owner)-[:OWNS]->(cal);

MATCH (admin:User {id: $bogota}), (cal:Calendar {id: $tech_meetup_cal})
MERGE (admin)-[:ADMINS {indexed_at: 1730000100000}]->(cal);

// Calendar 2: Community Events (owned by Cairo, co-admins Detroit and Eixample)
MERGE (c2:Calendar {id: $community_events_cal}) 
SET c2.uri = "pubky://" + $cairo + "/pub/eventky.app/calendars/" + $community_events_cal,
    c2.name = "Community Events",
    c2.description = "Open community gatherings and social events",
    c2.timezone = "Africa/Cairo",
    c2.color = "#33C4FF",
    c2.owner_id = $cairo,
    c2.created_at = 1730100000000,
    c2.indexed_at = 1730100000000;

MATCH (owner:User {id: $cairo}), (cal:Calendar {id: $community_events_cal})
MERGE (owner)-[:OWNS]->(cal);

MATCH (admin:User {id: $detroit}), (cal:Calendar {id: $community_events_cal})
MERGE (admin)-[:ADMINS {indexed_at: 1730100100000}]->(cal);

MATCH (admin:User {id: $eixample}), (cal:Calendar {id: $community_events_cal})
MERGE (admin)-[:ADMINS {indexed_at: 1730100200000}]->(cal);

// Calendar 3: Workshop Series (owned by Eixample, co-admin Amsterdam)
MERGE (c3:Calendar {id: $workshop_series_cal}) 
SET c3.uri = "pubky://" + $eixample + "/pub/eventky.app/calendars/" + $workshop_series_cal,
    c3.name = "Hands-on Workshops",
    c3.description = "Educational workshops for skill development",
    c3.timezone = "Europe/Madrid",
    c3.color = "#8B33FF",
    c3.owner_id = $eixample,
    c3.created_at = 1730200000000,
    c3.indexed_at = 1730200000000;

MATCH (owner:User {id: $eixample}), (cal:Calendar {id: $workshop_series_cal})
MERGE (owner)-[:OWNS]->(cal);

MATCH (admin:User {id: $amsterdam}), (cal:Calendar {id: $workshop_series_cal})
MERGE (admin)-[:ADMINS {indexed_at: 1730200100000}]->(cal);

// ##############################
// ##### Create Events ##########
// ##############################

// Event 1: In Tech Meetup Calendar (created by Amsterdam)
MERGE (e1:Event {id: $event1})
SET e1.uri = "pubky://" + $amsterdam + "/pub/eventky.app/events/" + $event1,
    e1.uid = "event-tech-meetup-001",
    e1.dtstamp = 1730300000000000,
    e1.sequence = 0,
    e1.summary = "Decentralized Web Technologies",
    e1.description = "Discussion on building decentralized applications",
    e1.dtstart = "2024-12-04T18:00:00",
    e1.dtend = "2024-12-04T20:00:00",
    e1.dtstart_tzid = "Europe/Amsterdam",
    e1.dtend_tzid = "Europe/Amsterdam",
    e1.location = "Tech Hub Amsterdam",
    e1.status = "CONFIRMED",
    e1.x_pubky_rsvp_access = "PUBLIC",
    e1.created_at = 1730300000000,
    e1.last_modified = 1730300000000,
    e1.indexed_at = 1730300000000;

MATCH (owner:User {id: $amsterdam}), (event:Event {id: $event1})
MERGE (owner)-[:AUTHORED]->(event);

MATCH (event:Event {id: $event1}), (cal:Calendar {id: $tech_meetup_cal})
MERGE (event)-[:BELONGS_TO]->(cal);

// Event 2: In Tech Meetup Calendar (created by Bogota)
MERGE (e2:Event {id: $event2})
SET e2.uri = "pubky://" + $bogota + "/pub/eventky.app/events/" + $event2,
    e2.uid = "event-tech-meetup-002",
    e2.dtstamp = 1730310000000000,
    e2.sequence = 0,
    e2.summary = "API Security Best Practices",
    e2.description = "Learn about securing your APIs",
    e2.dtstart = "2024-12-05T19:00:00",
    e2.dtend = "2024-12-05T21:00:00",
    e2.dtstart_tzid = "America/Bogota",
    e2.dtend_tzid = "America/Bogota",
    e2.location = "Online via Zoom",
    e2.status = "CONFIRMED",
    e2.x_pubky_rsvp_access = "PUBLIC",
    e2.created_at = 1730310000000,
    e2.last_modified = 1730310000000,
    e2.indexed_at = 1730310000000;

MATCH (owner:User {id: $bogota}), (event:Event {id: $event2})
MERGE (owner)-[:AUTHORED]->(event);

MATCH (event:Event {id: $event2}), (cal:Calendar {id: $tech_meetup_cal})
MERGE (event)-[:BELONGS_TO]->(cal);

// Event 3: In Community Events Calendar (created by Cairo)
MERGE (e3:Event {id: $event3})
SET e3.uri = "pubky://" + $cairo + "/pub/eventky.app/events/" + $event3,
    e3.uid = "event-community-001",
    e3.dtstamp = 1730320000000000,
    e3.sequence = 0,
    e3.summary = "Community Coffee Meetup",
    e3.description = "Casual coffee and networking",
    e3.dtstart = "2024-12-06T10:00:00",
    e3.dtend = "2024-12-06T12:00:00",
    e3.dtstart_tzid = "Africa/Cairo",
    e3.dtend_tzid = "Africa/Cairo",
    e3.location = "Downtown Café Cairo",
    e3.status = "CONFIRMED",
    e3.x_pubky_rsvp_access = "PUBLIC",
    e3.created_at = 1730320000000,
    e3.last_modified = 1730320000000,
    e3.indexed_at = 1730320000000;

MATCH (owner:User {id: $cairo}), (event:Event {id: $event3})
MERGE (owner)-[:AUTHORED]->(event);

MATCH (event:Event {id: $event3}), (cal:Calendar {id: $community_events_cal})
MERGE (event)-[:BELONGS_TO]->(cal);

// Event 4: In Community Events Calendar (created by Detroit)
MERGE (e4:Event {id: $event4})
SET e4.uri = "pubky://" + $detroit + "/pub/eventky.app/events/" + $event4,
    e4.uid = "event-community-002",
    e4.dtstamp = 1730330000000000,
    e4.sequence = 0,
    e4.summary = "Open Source Contribution Day",
    e4.description = "Contribute to open source projects together",
    e4.dtstart = "2024-12-07T14:00:00",
    e4.dtend = "2024-12-07T18:00:00",
    e4.dtstart_tzid = "America/Detroit",
    e4.dtend_tzid = "America/Detroit",
    e4.location = "Detroit Tech Center",
    e4.status = "TENTATIVE",
    e4.x_pubky_rsvp_access = "PUBLIC",
    e4.created_at = 1730330000000,
    e4.last_modified = 1730330000000,
    e4.indexed_at = 1730330000000;

MATCH (owner:User {id: $detroit}), (event:Event {id: $event4})
MERGE (owner)-[:AUTHORED]->(event);

MATCH (event:Event {id: $event4}), (cal:Calendar {id: $community_events_cal})
MERGE (event)-[:BELONGS_TO]->(cal);

// Event 5: In Workshop Series Calendar (created by Eixample)
MERGE (e5:Event {id: $event5})
SET e5.uri = "pubky://" + $eixample + "/pub/eventky.app/events/" + $event5,
    e5.uid = "event-workshop-001",
    e5.dtstamp = 1730340000000000,
    e5.sequence = 0,
    e5.summary = "Introduction to Rust Programming",
    e5.description = "Beginner-friendly Rust workshop",
    e5.dtstart = "2024-12-08T15:00:00",
    e5.dtend = "2024-12-08T17:30:00",
    e5.dtstart_tzid = "Europe/Madrid",
    e5.dtend_tzid = "Europe/Madrid",
    e5.location = "Barcelona Workshop Space",
    e5.status = "CONFIRMED",
    e5.x_pubky_rsvp_access = "PUBLIC",
    e5.created_at = 1730340000000,
    e5.last_modified = 1730340000000,
    e5.indexed_at = 1730340000000;

MATCH (owner:User {id: $eixample}), (event:Event {id: $event5})
MERGE (owner)-[:AUTHORED]->(event);

MATCH (event:Event {id: $event5}), (cal:Calendar {id: $workshop_series_cal})
MERGE (event)-[:BELONGS_TO]->(cal);

// Event 6: In Workshop Series Calendar (created by Amsterdam)
MERGE (e6:Event {id: $event6})
SET e6.uri = "pubky://" + $amsterdam + "/pub/eventky.app/events/" + $event6,
    e6.uid = "event-workshop-002",
    e6.dtstamp = 1730350000000000,
    e6.sequence = 0,
    e6.summary = "Web3 Development Workshop",
    e6.description = "Build your first dApp",
    e6.dtstart = "2024-12-09T16:00:00",
    e6.dtend = "2024-12-09T19:00:00",
    e6.dtstart_tzid = "Europe/Amsterdam",
    e6.dtend_tzid = "Europe/Amsterdam",
    e6.location = "Amsterdam Web3 Hub",
    e6.status = "CONFIRMED",
    e6.x_pubky_rsvp_access = "PUBLIC",
    e6.created_at = 1730350000000,
    e6.last_modified = 1730350000000,
    e6.indexed_at = 1730350000000;

MATCH (owner:User {id: $amsterdam}), (event:Event {id: $event6})
MERGE (owner)-[:AUTHORED]->(event);

MATCH (event:Event {id: $event6}), (cal:Calendar {id: $workshop_series_cal})
MERGE (event)-[:BELONGS_TO]->(cal);

// ##############################
// ##### Event Tags #############
// ##############################

// Tags for Event 1 (Decentralized Web Technologies) - tagged by multiple users
MATCH (u:User {id: $bogota}), (e:Event {id: $event1})
MERGE (u)-[:TAGGED {label: $tech_tag, id: "ETAG1A2B3C4D5E", indexed_at: 1730300100000}]->(e);

MATCH (u:User {id: $bogota}), (e:Event {id: $event1})
MERGE (u)-[:TAGGED {label: $decentralized_tag, id: "ETAG1F6G7H8I9J", indexed_at: 1730300100001}]->(e);

MATCH (u:User {id: $cairo}), (e:Event {id: $event1})
MERGE (u)-[:TAGGED {label: $web3_tag, id: "ETAG2K0L1M2N3O", indexed_at: 1730300200000}]->(e);

MATCH (u:User {id: $cairo}), (e:Event {id: $event1})
MERGE (u)-[:TAGGED {label: $tech_tag, id: "ETAG2P4Q5R6S7T", indexed_at: 1730300200001}]->(e);

MATCH (u:User {id: $detroit}), (e:Event {id: $event1})
MERGE (u)-[:TAGGED {label: $meetup_tag, id: "ETAG3U8V9W0X1Y", indexed_at: 1730300300000}]->(e);

// Tags for Event 2 (API Security) - tagged by multiple users
MATCH (u:User {id: $amsterdam}), (e:Event {id: $event2})
MERGE (u)-[:TAGGED {label: $api_tag, id: "ETAG4Z2A3B4C5D", indexed_at: 1730310100000}]->(e);

MATCH (u:User {id: $amsterdam}), (e:Event {id: $event2})
MERGE (u)-[:TAGGED {label: $security_tag, id: "ETAG4E6F7G8H9I", indexed_at: 1730310100001}]->(e);

MATCH (u:User {id: $detroit}), (e:Event {id: $event2})
MERGE (u)-[:TAGGED {label: $tech_tag, id: "ETAG5J0K1L2M3N", indexed_at: 1730310200000}]->(e);

MATCH (u:User {id: $detroit}), (e:Event {id: $event2})
MERGE (u)-[:TAGGED {label: $api_tag, id: "ETAG5O4P5Q6R7S", indexed_at: 1730310200001}]->(e);

// Tags for Event 3 (Coffee Meetup)
MATCH (u:User {id: $eixample}), (e:Event {id: $event3})
MERGE (u)-[:TAGGED {label: $community_tag, id: "ETAG6T8U9V0W1X", indexed_at: 1730320100000}]->(e);

MATCH (u:User {id: $eixample}), (e:Event {id: $event3})
MERGE (u)-[:TAGGED {label: $networking_tag, id: "ETAG6Y2Z3A4B5C", indexed_at: 1730320100001}]->(e);

MATCH (u:User {id: $amsterdam}), (e:Event {id: $event3})
MERGE (u)-[:TAGGED {label: $meetup_tag, id: "ETAG7D6E7F8G9H", indexed_at: 1730320200000}]->(e);

// Tags for Event 4 (Open Source Day)
MATCH (u:User {id: $cairo}), (e:Event {id: $event4})
MERGE (u)-[:TAGGED {label: $opensource_tag, id: "ETAG8I0J1K2L3M", indexed_at: 1730330100000}]->(e);

MATCH (u:User {id: $eixample}), (e:Event {id: $event4})
MERGE (u)-[:TAGGED {label: $community_tag, id: "ETAG8N4O5P6Q7R", indexed_at: 1730330200000}]->(e);

MATCH (u:User {id: $amsterdam}), (e:Event {id: $event4})
MERGE (u)-[:TAGGED {label: $opensource_tag, id: "ETAG9S8T9U0V1W", indexed_at: 1730330300000}]->(e);

// Tags for Event 5 (Rust Workshop)
MATCH (u:User {id: $amsterdam}), (e:Event {id: $event5})
MERGE (u)-[:TAGGED {label: $rust_tag, id: "ETAG10X2Y3Z4A5B", indexed_at: 1730340100000}]->(e);

MATCH (u:User {id: $amsterdam}), (e:Event {id: $event5})
MERGE (u)-[:TAGGED {label: $workshop_tag, id: "ETAG10C6D7E8F9G", indexed_at: 1730340100001}]->(e);

MATCH (u:User {id: $bogota}), (e:Event {id: $event5})
MERGE (u)-[:TAGGED {label: $tech_tag, id: "ETAG11H0I1J2K3L", indexed_at: 1730340200000}]->(e);

MATCH (u:User {id: $detroit}), (e:Event {id: $event5})
MERGE (u)-[:TAGGED {label: $rust_tag, id: "ETAG11M4N5O6P7Q", indexed_at: 1730340300000}]->(e);

// Tags for Event 6 (Web3 Workshop)
MATCH (u:User {id: $bogota}), (e:Event {id: $event6})
MERGE (u)-[:TAGGED {label: $web3_tag, id: "ETAG12R8S9T0U1V", indexed_at: 1730350100000}]->(e);

MATCH (u:User {id: $bogota}), (e:Event {id: $event6})
MERGE (u)-[:TAGGED {label: $workshop_tag, id: "ETAG12W2X3Y4Z5A", indexed_at: 1730350100001}]->(e);

MATCH (u:User {id: $cairo}), (e:Event {id: $event6})
MERGE (u)-[:TAGGED {label: $decentralized_tag, id: "ETAG13B6C7D8E9F", indexed_at: 1730350200000}]->(e);

MATCH (u:User {id: $detroit}), (e:Event {id: $event6})
MERGE (u)-[:TAGGED {label: $tech_tag, id: "ETAG13G0H1I2J3K", indexed_at: 1730350300000}]->(e);

// ##############################
// ##### Create Attendees #######
// ##############################

// Attendees for Event 1 (Decentralized Web Technologies) - 2 attendees
MERGE (a1:Attendee {id: $att1_evt1})
SET a1.partstat = "ACCEPTED",
    a1.created_at = 1730300100000,
    a1.indexed_at = 1730300100000;

MATCH (user:User {id: $bogota}), (att:Attendee {id: $att1_evt1})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att1_evt1}), (event:Event {id: $event1})
MERGE (att)-[:RSVP_TO]->(event);

MERGE (a2:Attendee {id: $att2_evt1})
SET a2.partstat = "TENTATIVE",
    a2.created_at = 1730300200000,
    a2.indexed_at = 1730300200000;

MATCH (user:User {id: $cairo}), (att:Attendee {id: $att2_evt1})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att2_evt1}), (event:Event {id: $event1})
MERGE (att)-[:RSVP_TO]->(event);

// Attendees for Event 2 (API Security) - 2 attendees
MERGE (a3:Attendee {id: $att3_evt2})
SET a3.partstat = "ACCEPTED",
    a3.created_at = 1730310100000,
    a3.indexed_at = 1730310100000;

MATCH (user:User {id: $amsterdam}), (att:Attendee {id: $att3_evt2})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att3_evt2}), (event:Event {id: $event2})
MERGE (att)-[:RSVP_TO]->(event);

MERGE (a4:Attendee {id: $att4_evt2})
SET a4.partstat = "ACCEPTED",
    a4.created_at = 1730310200000,
    a4.indexed_at = 1730310200000;

MATCH (user:User {id: $detroit}), (att:Attendee {id: $att4_evt2})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att4_evt2}), (event:Event {id: $event2})
MERGE (att)-[:RSVP_TO]->(event);

// Attendees for Event 3 (Coffee Meetup) - 2 attendees
MERGE (a5:Attendee {id: $att5_evt3})
SET a5.partstat = "ACCEPTED",
    a5.created_at = 1730320100000,
    a5.indexed_at = 1730320100000;

MATCH (user:User {id: $eixample}), (att:Attendee {id: $att5_evt3})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att5_evt3}), (event:Event {id: $event3})
MERGE (att)-[:RSVP_TO]->(event);

MERGE (a9:Attendee {id: $att9_evt3})
SET a9.partstat = "ACCEPTED",
    a9.created_at = 1730320150000,
    a9.indexed_at = 1730320150000;

MATCH (user:User {id: $amsterdam}), (att:Attendee {id: $att9_evt3})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att9_evt3}), (event:Event {id: $event3})
MERGE (att)-[:RSVP_TO]->(event);

// Attendees for Event 4 (Open Source Day) - 2 attendees
MERGE (a6:Attendee {id: $att6_evt4})
SET a6.partstat = "DECLINED",
    a6.created_at = 1730330100000,
    a6.indexed_at = 1730330100000;

MATCH (user:User {id: $cairo}), (att:Attendee {id: $att6_evt4})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att6_evt4}), (event:Event {id: $event4})
MERGE (att)-[:RSVP_TO]->(event);

MERGE (a10:Attendee {id: $att10_evt4})
SET a10.partstat = "TENTATIVE",
    a10.created_at = 1730330150000,
    a10.indexed_at = 1730330150000;

MATCH (user:User {id: $eixample}), (att:Attendee {id: $att10_evt4})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att10_evt4}), (event:Event {id: $event4})
MERGE (att)-[:RSVP_TO]->(event);

// Attendees for Event 5 (Rust Workshop) - 2 attendees
MERGE (a7:Attendee {id: $att7_evt5})
SET a7.partstat = "ACCEPTED",
    a7.created_at = 1730340100000,
    a7.indexed_at = 1730340100000;

MATCH (user:User {id: $amsterdam}), (att:Attendee {id: $att7_evt5})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att7_evt5}), (event:Event {id: $event5})
MERGE (att)-[:RSVP_TO]->(event);

MERGE (a11:Attendee {id: $att11_evt5})
SET a11.partstat = "ACCEPTED",
    a11.created_at = 1730340150000,
    a11.indexed_at = 1730340150000;

MATCH (user:User {id: $bogota}), (att:Attendee {id: $att11_evt5})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att11_evt5}), (event:Event {id: $event5})
MERGE (att)-[:RSVP_TO]->(event);

// Attendees for Event 6 (Web3 Workshop) - 2 attendees
MERGE (a8:Attendee {id: $att8_evt6})
SET a8.partstat = "NEEDS-ACTION",
    a8.created_at = 1730350100000,
    a8.indexed_at = 1730350100000;

MATCH (user:User {id: $bogota}), (att:Attendee {id: $att8_evt6})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att8_evt6}), (event:Event {id: $event6})
MERGE (att)-[:RSVP_TO]->(event);

MERGE (a12:Attendee {id: $att12_evt6})
SET a12.partstat = "ACCEPTED",
    a12.created_at = 1730350150000,
    a12.indexed_at = 1730350150000;

MATCH (user:User {id: $cairo}), (att:Attendee {id: $att12_evt6})
MERGE (user)-[:AUTHORED]->(att);

MATCH (att:Attendee {id: $att12_evt6}), (event:Event {id: $event6})
MERGE (att)-[:RSVP_TO]->(event);

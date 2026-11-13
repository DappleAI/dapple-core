SELECT g.conduits_md
FROM rev.dispatches d
JOIN rev.journeys j ON j.id = d.journey_id
JOIN rev.guides g ON g.id = j.guide_id
WHERE d.id = $1::UUID;

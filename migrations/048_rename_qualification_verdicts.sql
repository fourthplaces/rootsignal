-- Rename qualification verdicts: green/yellow/red → approved/review/declined
UPDATE sources SET qualification_status = 'approved' WHERE qualification_status = 'green';
UPDATE sources SET qualification_status = 'review' WHERE qualification_status = 'yellow';
UPDATE sources SET qualification_status = 'declined' WHERE qualification_status = 'red';

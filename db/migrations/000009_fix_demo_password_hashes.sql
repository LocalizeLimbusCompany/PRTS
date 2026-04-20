UPDATE users
SET password_hash = '$2a$10$VRt8Z35mWSN.dCJNSj3f8OQ2XE/mSsZ/rVg4oHdtsZu6zfckxNfXa'
WHERE email = 'admin@example.com'
  AND password_hash IN (
    '$2a$10$7EqJtq98hPqEX7fNZaFWoO5NLqC8oWQz1xM0t1Ik5dUvNbK/C.j7G',
    'admin123',
    ''
  );

UPDATE users
SET password_hash = '$2a$10$tm/ecu/2Co4MMCbOjpPzQOARuvMGwnmtXHv9QAAPg27Ea5dBrO1a6'
WHERE email = 'reviewer@example.com'
  AND password_hash IN (
    '$2a$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy',
    'reviewer123',
    ''
  );

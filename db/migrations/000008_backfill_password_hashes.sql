UPDATE users
SET password_hash = '$2a$10$7EqJtq98hPqEX7fNZaFWoO5NLqC8oWQz1xM0t1Ik5dUvNbK/C.j7G'
WHERE email = 'admin@example.com'
  AND (password_hash = 'admin123' OR password_hash = '');

UPDATE users
SET password_hash = '$2a$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy'
WHERE email = 'reviewer@example.com'
  AND (password_hash = 'reviewer123' OR password_hash = '');

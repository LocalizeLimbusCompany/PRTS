UPDATE users
SET platform_role = CASE
    WHEN email = 'admin@example.com' THEN 'owner'
    ELSE platform_role
END;

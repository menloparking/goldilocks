# String interpolation and heredocs

# Simple interpolation — leave alone

# Long interpolation that exceeds 100 cols — break after spaces

# Long plain string — no interpolation

# Short string — fits, leave alone

# String with no spaces (pathological) — cannot be broken, leave it

# Heredoc — completely untouchable
puts <<~SQL
  SELECT users.*, COUNT(posts.id) AS post_count
  FROM users
  LEFT JOIN posts ON posts.user_id = users.id
  WHERE users.active = true
  GROUP BY users.id
  HAVING COUNT(posts.id) > 5
  ORDER BY post_count DESC
SQL

# Heredoc in method call
execute(<<~SQL, user.id)
  UPDATE users SET last_login_at = NOW() WHERE id = $1
SQL

# Percent strings

# String with complex interpolation that needs internal formatting
"User #{User
  .includes(:profile)
  .where(active: true, verified: true)
  .order(created_at: :desc)
  .first
  .email} logged in from #{request.remote_ip} at #{Time.current.strftime("%Y-%m-%d %H:%M:%S")}"

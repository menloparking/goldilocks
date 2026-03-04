# Boundary tests: max_width is 100; lines of exactly 100 chars stay flat, 101+ chars break.

# ── Method calls (parenthesized) ──────────────────────────────────────────────

# Flat output = exactly 100 chars → should stay on one line
User.create(first_name: "Aliceeeee", last_name: "Wonderland", email: "alice@wonderland.example.com")

# Flat output = exactly 101 chars → should break
User.create(
  first_name: "Aliceeeeee",
  last_name: "Wonderland",
  email: "alice@wonderland.example.com"
)

# Another method call: flat = exactly 100 chars
send_notification(user: current_user, message: "Your order has been shipped", priority: :hi_urgents)

# Another method call: flat = exactly 101 chars
send_notification(
  user: current_user,
  message: "Your order has been shipped",
  priority: :hi_urgentss
)

# ── Hash literals ─────────────────────────────────────────────────────────────

# Hash assignment: flat = exactly 100 chars → should stay on one line
_h = {first_name: "Alice", last_name: "Wonderland", email: "alice@wonderland.example.com", age: 300}

# Hash assignment: flat = exactly 101 chars → should break
_h = {
  first_name: "Alice",
  last_name: "Wonderland",
  email: "alice@wonderland.example.com",
  age: 3000
}

# ── Array literals ────────────────────────────────────────────────────────────

# Array assignment: flat = exactly 100 chars → should stay on one line
_a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 27]

# Array assignment: flat = exactly 101 chars → should break
_a = [
  1,
  2,
  3,
  4,
  5,
  6,
  7,
  8,
  9,
  10,
  11,
  12,
  13,
  14,
  15,
  16,
  17,
  18,
  19,
  20,
  21,
  22,
  23,
  24,
  25,
  270
]

# ── 3-segment method chains ──────────────────────────────────────────────────

# 3-segment chain: flat = exactly 100 chars → should stay on one line
users.where(active: true).order(created_at: :desc).includes(:profile, :settings, :notification_pref)

# 3-segment chain: flat = exactly 101 chars → should break
users
  .where(active: true)
  .order(created_at: :desc)
  .includes(:profile, :settings, :notification_prefs)

# ── 4+ segment method chains (always force-break regardless of width) ─────────

# 4+ segments at only 85 chars — still must break (force-break rule)
users
  .where(active: true)
  .order(:name)
  .includes(:profile)
  .limit(50)

# ── Indented boundary (inside def, 2-space indent) ───────────────────────────

# Content = 98 chars, + 2 indent = 100 total → should stay flat
def indented_boundary_flat
  User.create(first_name: "Aliceee", last_name: "Wonderland", email: "alice@wonderland.example.com")
end

# Content = 99 chars, + 2 indent = 101 total → should break
def indented_boundary_break
  User.create(
    first_name: "Aliceeee",
    last_name: "Wonderland",
    email: "alice@wonderland.example.com"
  )
end

# ── Deeply indented (class > def, 4-space indent) ────────────────────────────

class BoundaryTest
  # Content = 96 chars, + 4 indent = 100 total → should stay flat
  def deeply_indented_flat
    User.create(first_name: "Alice", last_name: "Wonderland", email: "alice@wonderland.example.com")
  end

  # Content = 97 chars, + 4 indent = 101 total → should break
  def deeply_indented_break
    User.create(
      first_name: "Alicee",
      last_name: "Wonderland",
      email: "alice@wonderland.example.com"
    )
  end
end

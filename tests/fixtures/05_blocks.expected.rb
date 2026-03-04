# Blocks: do..end and braces

# Short brace block — stays on one line
users.each { |u| u.activate! }

# Multiline do..end with bad indentation
users.each do |user|
  user.activate!
  user.notify
end

# Long block that needs rewrap
User
  .where(active: true)
  .includes(:profile, :settings)
  .order(created_at: :desc)
  .each do |user|
    UserMailer.weekly_digest(user).deliver_later
  end

# Block with long args
items.map do |item|
  {
    id: item.id,
    name: item.name,
    description: item.description,
    category: item.category,
    price: item.price,
    quantity: item.quantity
  }
end

# Nested blocks
users.group_by(&:role).each do |_role, group|
  group.each do |user|
    process(user)
  end
end

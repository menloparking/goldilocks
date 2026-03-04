# Method chains — the classic rewrap challenge

# Short chain — fits on one line
users.active.verified.count

# Medium chain — still fits
User.where(active: true).order(:name).limit(10)

# Long chain that must be broken across lines
User
  .includes(:profile, :posts)
  .where(active: true, verified: true)
  .where
  .not(banned: true)
  .order(created_at: :desc)
  .limit(25)
  .offset(50)

# Rails-style scope chain
Post
  .published
  .where(author: current_user)
  .includes(:comments, :tags)
  .order(published_at: :desc)
  .page(params[:page])
  .per(20)

# Chain with block at end
User
  .where(role: :admin)
  .includes(:permissions)
  .order(:last_login_at)
  .each do |admin|
    admin.audit_access!
  end

# Chain on separate lines already but with bad indentation
User
  .where(active: true)
  .includes(:profile)
  .order(:name)
  .limit(10)

# Very long single method in chain
Article
  .where(
    "published_at > ? AND category_id IN (?) AND status = ?",
    1.week.ago,
    Category.featured.pluck(:id),
    "approved"
  )
  .includes(:author, :comments)
  .order(published_at: :desc)

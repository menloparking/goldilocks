# Short method — should stay on one line if it fits
def greet(name)
  "Hello, #{name}!"
end

# Method with many params — fits in 100 cols, leave it
def connect(host, port, timeout, retries)
  # body
end

# Method with params that exceed 100 columns — must rewrap
def send_notification(
  user_email,
  subject_line,
  body_text,
  priority_level,
  attachment_path,
  cc_list,
  bcc_list
)
  Mailer.deliver(
    to: user_email,
    subject: subject_line,
    body: body_text,
    priority: priority_level,
    attachment: attachment_path,
    cc: cc_list,
    bcc: bcc_list
  )
end

# Method with default values that exceed 100 columns
def configure_database(
  adapter = "postgresql",
  host = "localhost",
  port = 5432,
  database = "myapp_development",
  pool_size = 10,
  timeout = 5000
)
  # body
end

# Bad indentation
def messy
  3
end

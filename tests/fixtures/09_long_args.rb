# Long argument lists in method calls

# Short call — fits on one line
User.find(42)

# Medium call — still fits
User.create(name: 'Alice', email: 'alice@example.com')

# Long call that needs rewrap
User.create(first_name: 'Alice', last_name: 'Wonderland', email: 'alice@wonderland.example.com',
            password: 's3cure_p4ssw0rd!', role: :admin, department: 'Engineering', start_date: Date.today)

# Nested method calls that are too long
send_welcome_email(User.find_by(email: params[:email], active: true),
                   generate_token(length: 32, charset: :alphanumeric), { template: 'welcome', priority: :high, track_opens: true, track_clicks: true })

# Method call with splat and keyword args
def create_record(*args, validate: true, callbacks: true, touch: true, **options)
  # body
end

# Super long string argument
logger.info("Processing batch job ##{job.id}: #{job.items.count} items queued for #{job.queue_name} with priority #{job.priority} at #{Time.current}")

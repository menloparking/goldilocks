# Conditionals: various forms

# Normal if/elsif/else with bad indentation
if x > 0
  puts 'positive'
elsif x == 0
  puts 'zero'
else
  puts 'negative'
end

# Postfix if — short enough to stay postfix
return if user.nil?

# Ternary — short enough to stay on one line
status = active? ? 'on' : 'off'

# Long ternary that should probably become an if/else
result = if some_really_long_condition_that_checks_many_things?(arg1, arg2,
                                                                arg3)
           'this is the truthy value string'
         else
           'this is the falsy value string'
         end
puts result

# Unless
raise 'Unauthorized' unless user.admin?

# Case/when
case status
when :active
  activate_user
when :suspended, :banned
  deactivate_user
else
  raise "Unknown status: #{status}"
end

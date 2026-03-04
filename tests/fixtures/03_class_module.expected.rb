# Classes and modules with bad indentation and spacing
module MyApp
  module Services
    class UserNotifier < BaseNotifier
      include Logging
      extend ClassMethods

      attr_reader :user, :config

      def initialize(user, config = {})
        @user = user
        @config = config
      end

      def notify
        send_email
        log("notified")
      end
    end
  end
end

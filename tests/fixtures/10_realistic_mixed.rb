# Realistic mixed example — a Rails controller action

class OrdersController < ApplicationController
  before_action :authenticate_user!
  before_action :set_order, only: %i[show edit update destroy]

  def index
    @orders = current_user.orders.includes(:line_items,
                                           :shipping_address).where(status: %i[pending processing
                                                                               shipped]).order(created_at: :desc).page(params[:page]).per(25)

    respond_to do |format|
      format.html
      format.json { render json: @orders, each_serializer: OrderSerializer, include: 'line_items' }
    end
  end

  def create
    @order = current_user.orders.build(order_params)

    if @order.save
      OrderMailer.confirmation(@order).deliver_later
      redirect_to @order, notice: 'Order was successfully created.'
    else
      render :new, status: :unprocessable_entity
    end
  end

  private

  def set_order
    @order = current_user.orders.find(params[:id])
  end

  def order_params
    params.require(:order).permit(:shipping_address_id, :billing_address_id, :payment_method_id, :notes, :gift_wrap,
                                  :gift_message, line_items_attributes: %i[product_id quantity variant_id _destroy])
  end
end

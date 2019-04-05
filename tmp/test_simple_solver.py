import unittest
from tmp.simple_solver import simple_solve, Order
from tmp.test_data import TOKENS, SAMPLE_ORDERS, TYPE_II_ORDERS, TYPE_I_A, TYPE_I_B


class TestSimpleSolve(unittest.TestCase):

    def test_retreat_example(self):
        order_list = list(map(lambda t: Order.from_dictionary(t), SAMPLE_ORDERS))
        simple_solve(order_list, TOKENS)

    def test_type_II(self):
        order_list = list(map(lambda t: Order.from_dictionary(t), TYPE_II_ORDERS))
        simple_solve(order_list, TOKENS)

    def test_type_I_A(self):
        order_list = list(map(lambda t: Order.from_dictionary(t), TYPE_I_A))
        simple_solve(order_list, TOKENS)

    def test_type_I_B(self):
        order_list = list(map(lambda t: Order.from_dictionary(t), TYPE_I_B))
        simple_solve(order_list, TOKENS)

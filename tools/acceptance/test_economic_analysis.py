from fractions import Fraction
import unittest

import economic_analysis as model


class EconomicAnalysis(unittest.TestCase):
    def test_native_weight_boundaries(self):
        for age, factor in [(0, 1), (Fraction(1, 2), Fraction(5, 4)), (1, Fraction(3, 2)),
                            (2, 2), (3, Fraction(5, 2)), (4, 3), (7, 3)]:
            self.assertEqual(model.weight(1, age), 2 * factor)
        self.assertEqual(model.weight(1, 0, Fraction(1, 2)), Fraction(3, 2))
        self.assertEqual(model.weight(1, 4, Fraction(1, 4)), 0)

    def test_inventory_is_conserved_and_common_factors_cancel(self):
        weights = [model.weight(100, 4), model.weight(100, 0)]
        self.assertEqual(model.allocate(1000, weights), [750, 250])
        self.assertEqual(model.allocate(1000, [w * 10 for w in weights]), [750, 250])
        self.assertEqual(model.allocate(0, weights), [0, 0])
        self.assertEqual(model.allocate(1000, [0, 0]), [0, 0])
        self.assertEqual(model.allocate(1000, [model.weight(100, 4)] * 2), [500, 500])
        self.assertEqual(sum(model.allocate(1000, weights + [model.weight(1000, 0)])), 1000)
        # Equal additional proposal participation/stake changes cannot create inventory.
        self.assertEqual(sum(model.allocate(1000, [w * 100 for w in weights])), 1000)
        # Splitting is neutral before real fees/rounding and cannot copy past votes.
        self.assertEqual(model.weight(40, 4) + model.weight(60, 4), model.weight(100, 4))

    def test_exclusions_change_shares_without_creating_tokens(self):
        external = [model.weight(100, 4), model.weight(100, 0)]
        included = model.allocate(1000, external + [model.weight(10_000_000, 4)])
        excluded = model.allocate(1000, external)
        self.assertGreater(excluded[0], included[0])
        self.assertEqual(sum(excluded), sum(included))
        for row in model.scenarios():
            self.assertEqual(sum(map(Fraction, row['external_inventory_allocations'])),
                             row['assumptions']['settled_inventory_units'])


if __name__ == '__main__':
    unittest.main()

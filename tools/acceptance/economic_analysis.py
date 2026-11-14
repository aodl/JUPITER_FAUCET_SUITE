"""Small deterministic incentive model; assumptions, not income/price forecasts.

Native mechanics are proved separately by PocketIC. Inventory is a labelled
settlement-inventory arithmetic fixture, never an asserted IO receipt/entitlement.
"""
from fractions import Fraction
import json

import economics


def weight(stake, age_years, delay_years=1, participation=1):
    e = economics.load()
    delay = min(Fraction(delay_years), Fraction(e['max_dissolve_delay_seconds'], e['year_seconds']))
    if delay < Fraction(e['minimum_voting_dissolve_delay_seconds'], e['year_seconds']):
        return Fraction()
    age = min(Fraction(age_years), Fraction(e['max_neuron_age_for_age_bonus'], e['year_seconds']))
    max_delay = Fraction(e['max_dissolve_delay_seconds'], e['year_seconds'])
    max_age = Fraction(e['max_neuron_age_for_age_bonus'], e['year_seconds'])
    return (Fraction(stake) * (1 + delay / max_delay * Fraction(e['dissolve_bonus_percentage'], 100)) *
            (1 + age / max_age * Fraction(e['age_bonus_percentage'], 100)) * Fraction(participation))


def allocate(inventory, weights):
    total = sum(weights, Fraction())
    return [Fraction(inventory) * w / total if total else Fraction() for w in weights]


def scenarios():
    # Every amount is an explicit hypothetical input in jUP or abstract IO units.
    # Protocol stake is always excluded from the external economic denominator.
    definitions = [
        ('small sale, slow growth', 5_000, 4, 5_000, 0, 1_000),
        ('large late productive ICP, conditional inventory', 5_000, 4, 100_000, 0, 10_000),
        ('rapid reserve distribution and staking', 5_000, 4, 80_000_000, 0, 1_000),
        ('years of zero settled IO', 5_000, 4, 5_000, 0, 0),
        ('later increasing authenticated inventory', 5_000, 4, 5_000, 0, 10_000),
        ('many old neurons, few new participants', 1_000_000, 4, 5_000, 0, 1_000),
        ('many new participants', 5_000, 4, 1_000_000, 0, 1_000),
        ('NNS principal top-up and diluted age', 5_000, 4, 5_000, 0, 1_000),
        ('starts dissolution versus keeps one-year delay', 5_000, 0, 5_000, 4, 1_000),
        ('genuine transfer of aged economic control', 5_000, 4, 5_000, 0, 1_000),
    ]
    output = []
    for name, early_stake, early_age, late_stake, late_age, inventory in definitions:
        early, late = weight(early_stake, early_age), weight(late_stake, late_age)
        protocol = weight(10_000_000, 4)
        amounts = allocate(inventory, [early, late])
        output.append(dict(scenario=name, assumptions=dict(early_jup=early_stake, early_age_years=early_age,
            late_jup=late_stake, late_age_years=late_age, settled_inventory_units=inventory,
            protocol_jup=10_000_000, protocol_age_years=4, delay_years=1, equal_activity=True),
            protocol_voting_fraction=str(protocol / (protocol + early + late)),
            early_external_fraction=str(early / (early + late)),
            early_to_late_weight=str(early / late), external_inventory_allocations=[str(a) for a in amounts]))
    return output


if __name__ == '__main__':
    print(json.dumps({'status': 'hypothetical deterministic analysis; no market, yield or income prediction',
        'scenarios': scenarios(), 'nns_top_up': {'old_principal_icp': 100, 'new_principal_icp': 900,
        'old_age_years': 4, 'weighted_age_after_years': '2/5',
        'note': 'Native stake-weighted age; future maturity and IO inventory are not predicted.'}}, indent=2))

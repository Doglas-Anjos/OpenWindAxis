"""Independent cross-check of wind-core against the published formulas, with the
Python standard library only (no numpy/scipy), so it runs anywhere.

It reads the CSV on its own (not through the core's importer), recomputes each
figure from its definition, runs the same analyses through the `wind` command
line and fails on any disagreement beyond the stated tolerance.

Usage (after `cargo build --release -p wind-cli`):
    python bench/crosscheck.py samples/torre_exemplo.csv target/release/wind
"""
import calendar
import csv
import json
import math
from bisect import bisect_right
import os
import statistics
import subprocess
import sys
from collections import defaultdict
from datetime import datetime

# ---- The sample file's layout ----
PREAMBLE_LINES = 3
DELIMITER = ';'
MISSING = '-999'
TIME_FORMAT = '%d/%m/%Y %H:%M'
SPEED_TOP, SPEED_BOTTOM, DIRECTION = 'Vel 100m Média', 'Vel 80m Média', 'Dir 98m'
SPEED_TOP_SD = 'Vel 100m DP'
HEIGHT_TOP_M, HEIGHT_BOTTOM_M = 100.0, 80.0
STEP_S = 600
STEPS_PER_DAY = 86_400 // STEP_S

# ---- Formulas ----
RHO_STD = 1.225  # ISO 2533 sea-level air density, kg/m³
HALF = 0.5
ROSE_SECTORS = 16
DEGREES_PER_TURN = 360.0
DIURNAL_HOURS_CHECKED = (0, 6, 12, 18)
# Mean days per calendar month, as the mean of monthly means weighs them.
MEAN_DAYS = (31.0, 28.24, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0)
SHAPE_BRACKET = (0.3, 20.0)
# Turbulence (IEC 61400-1): TI = SD / mean from 4 m/s; the 15 m/s bin; the
# representative TI as the normal 90 % quantile; NTM I_ref·(0.75 + 5.6/U) at
# 15 m/s for categories C, B, A and A+ (fourth edition).
TI_MIN_SPEED = 4.0
BIN_15 = (14.5, 15.5)
QUANTILE_90 = 1.28
NTM_SLOPE, NTM_OFFSET, REFERENCE_SPEED = 0.75, 5.6, 15.0
CATEGORIES = (('C', 0.12), ('B', 0.14), ('A', 0.16), ('A+', 0.18))
# Extremes: monthly maxima of months at least half complete, a Gumbel fit by
# maximum likelihood, annualized over 12 months, the 50-year wind.
MIN_RECOVERY = 0.5
MONTHS_PER_YEAR = 12
DESIGN_YEARS = 50.0
BISECTION_STEPS = 200

# ---- Tolerances (relative unless named absolute) ----
FIT_TOLERANCE = 1e-6  # both sides solve to ~1e-12; the gap is the solvers'
# The original's maximum likelihood (manual articles-weibull-ml.htm; details
# from its code): Stevens & Smulders iterated from k = 2 until k changes less
# than 0.1 % (k at most 10); speeds below the 0.5th percentile, read from a
# 200-point quantile curve and never below the smallest positive speed, are
# drawn again from the fit below that cutoff, up to 20 rounds, with Numerical
# Recipes' ran0 seeded at -4723.
FIRST_SHAPE, MAX_SHAPE, SHAPE_TOLERANCE, MAX_STEPS = 2.0, 10.0, 1e-3, 100
LOW_SPEED_PERCENT, CURVE_POINTS, CURVE_GAP, MAX_ROUNDS, DRAW_SEED = 0.5, 200, 1e-5, 20, -4723
RAN0_MULTIPLIER, RAN0_MODULUS, RAN0_QUOTIENT, RAN0_MASK = 16807, 2147483647, 127773, 0x075BD924
# Least squares: each speed at (i + ½)/n among all n, held within these.
PLOTTING_POSITION_RANGE = (1e-6, 1 - 1e-6)
SUM_TOLERANCE = 1e-9  # same sums in a different order
SHARE_ABSOLUTE = 1e-12
DEVIATION_TOLERANCE = 1e-8  # running variance against exact rational arithmetic


def read(path):
    """Rows of the sample: (time, {column: value or None})."""
    with open(path, encoding='utf-8') as source:
        lines = source.read().splitlines()[PREAMBLE_LINES:]
    reader = csv.reader(lines, delimiter=DELIMITER)
    header = next(reader)
    rows = []
    for record in reader:
        stamp = datetime.strptime(record[0], TIME_FORMAT)
        values = {}
        for name, field in zip(header[1:], record[1:]):
            values[name] = None if field in ('', MISSING) else float(field.replace(',', '.'))
        rows.append((stamp, values))
    return rows


def bisect(function, low, high):
    """Root of `function` on [low, high], which brackets a sign change."""
    f_low = function(low)
    for _ in range(BISECTION_STEPS):
        middle = (low + high) / 2
        if (function(middle) < 0) == (f_low < 0):
            low = middle
        else:
            high = middle
    return (low + high) / 2


class Ran0:
    """Numerical Recipes' ran0 (Park & Miller's minimal standard, masked)."""

    def __init__(self, seed):
        self.state = seed

    def next(self):
        state = self.state ^ RAN0_MASK
        state = RAN0_MULTIPLIER * state - RAN0_MODULUS * int(state / RAN0_QUOTIENT)
        if state < 0:
            state += RAN0_MODULUS
        self.state = state ^ RAN0_MASK
        return state / RAN0_MODULUS


def stevens_smulders(speeds):
    """k iterated over the positive speeds; A over every speed."""
    positive = [speed for speed in speeds if speed > 0]
    logs = [math.log(speed) for speed in positive]
    mean_log = sum(logs) / len(logs)
    shape = FIRST_SHAPE
    for _ in range(MAX_STEPS):
        powers = [speed ** shape for speed in positive]
        following = min(1 / (sum(p * log for p, log in zip(powers, logs)) / sum(powers) - mean_log), MAX_SHAPE)
        change = abs(following - shape) / shape
        shape = following
        if change < SHAPE_TOLERANCE:
            break
    return shape, (sum(speed ** shape for speed in speeds) / len(speeds)) ** (1 / shape)


def likelihood_cutoff(speeds):
    """The 0.5th percentile on a 200-point quantile curve, at least the smallest positive speed."""
    ordered = sorted(speeds)
    count = len(ordered)
    points = min(CURVE_POINTS, count)
    curve = []
    for point in range(points):
        rank = int((count - 1) * point / (points - 1) + 0.5)
        curve.append(((rank + 0.5) * 100 / count, ordered[rank]))
    if LOW_SPEED_PERCENT <= curve[0][0]:
        value = curve[0][1]
    elif LOW_SPEED_PERCENT >= curve[-1][0]:
        value = curve[-1][1]
    else:
        upper = next(i for i, (percent, _) in enumerate(curve) if percent > LOW_SPEED_PERCENT)
        (low_percent, low_speed), (high_percent, high_speed) = curve[upper - 1], curve[upper]
        if abs(high_percent - low_percent) < CURVE_GAP:
            value = (low_speed + high_speed) / 2
        else:
            share = (LOW_SPEED_PERCENT - low_percent) / (high_percent - low_percent)
            value = low_speed * (1 - share) + high_speed * share
    return max(value, min(speed for speed in speeds if speed > 0))


def max_likelihood(speeds):
    cutoff = likelihood_cutoff(speeds)
    high = [speed for speed in speeds if speed >= cutoff]
    low = len(speeds) - len(high)
    shape, scale = stevens_smulders(high)
    generator = Ran0(DRAW_SEED)
    for _ in range(MAX_ROUNDS if low else 0):
        below = 1 - math.exp(-((cutoff / scale) ** shape))
        drawn = [scale * (-math.log(1 - generator.next() * below)) ** (1 / shape) for _ in range(low)]
        following, scale = stevens_smulders(high + drawn)
        change = abs(following - shape) / shape
        shape = following
        if change < SHAPE_TOLERANCE:
            break
    return shape, scale


def least_squares(speeds):
    """Line through (ln u, ln(-ln(1 - F))), F = (i + ½)/n held in range; zeros off the line."""
    ordered = sorted(speeds)
    low, high = PLOTTING_POSITION_RANGE
    points = [(math.log(speed), math.log(-math.log(1 - min(max((rank + 0.5) / len(ordered), low), high))))
              for rank, speed in enumerate(ordered) if speed > 0]
    mean_x = sum(x for x, _ in points) / len(points)
    mean_y = sum(y for _, y in points) / len(points)
    shape = sum((x - mean_x) * (y - mean_y) for x, y in points) / sum((x - mean_x) ** 2 for x, _ in points)
    return shape, math.exp(-(mean_y - shape * mean_x) / shape)


# The speed distribution's R² (Analisar › Distribuição de velocidade), as the
# original lays its bins: a span widened to round axis ticks, cut into 40 to
# 80 bins from 0; observed against fitted percent per bin.
SAME_TOLERANCE, LEAST_BINS, MOST_BINS, FALLBACK_BINS = 1e-6, 40, 80, 4


def same(left, right):
    if left == 0:
        return abs(right) <= SAME_TOLERANCE
    if right == 0:
        return abs(left) <= SAME_TOLERANCE
    return abs((left - right) / left) <= SAME_TOLERANCE


def ticks(low, high):
    if low == 0 and high == 0:
        high = 1.0
    elif low == high:
        high = low + 1.0
    span = high - low
    power = 10.0 ** math.floor(math.log10(span))
    scaled = span * 10.0 / power
    multiple_of_four = lambda value: same(value, round(value * 0.25) * 4.0)
    if multiple_of_four(low) and multiple_of_four(high) and scaled * 0.25 in (3.0, 4.0, 6.0, 8.0):
        major, minor = scaled * 0.25, 1.0
    else:
        fifth = scaled / 5.0
        major, minor = ((2.0, 1.0) if fifth < 3 else (5.0, 1.0) if fifth == 3 else (4.0, 1.0) if fifth < 4
                        else (5.0, 1.0) if fifth < 7.5 else (10.0, 5.0) if fifth < 15 else (20.0, 5.0))
    return power * major / 10.0, power * minor / 10.0


def distribution_r_squared(speeds, shape, scale):
    low, high = min(speeds), max(speeds)
    for _ in range(2):
        major = ticks(low, high)[0]
        low = math.floor(low / major) * major
        ratio = high / major
        high = high if same(ratio, round(ratio)) else (int(ratio) + 1) * major
    count = int(round((high - low) / ticks(low, high)[1])) or FALLBACK_BINS
    while count < LEAST_BINS:
        count *= 2
    while count > MOST_BINS:
        count = count // 3 if count % 2 and count % 3 == 0 else count // 5 if count % 2 and count % 5 == 0 else count // 2
    width = (high - low) / count
    ordered = sorted(speeds)
    edges = [index * width for index in range(count + 1)]
    observed = [bisect_right(ordered, edge) * 100.0 / len(ordered) for edge in edges]
    fitted = [0.0 if edge <= 0 else (1 - math.exp(-((edge / scale) ** shape))) * 100.0 for edge in edges]
    shares = lambda cumulative: [cumulative[index + 1] - cumulative[index] for index in range(count)]
    measured, curve = shares(observed), shares(fitted)
    mean = sum(measured) / count
    return 1 - sum((m - c) ** 2 for m, c in zip(measured, curve)) / sum((m - mean) ** 2 for m in measured)


def weibull_fits(speeds):
    """k and A by maximum likelihood and least squares (as the original fits
    them), Openwind (mean and mean cube) and WAsP (mean cube and the share
    above the mean)."""
    count = len(speeds)
    mean = sum(speeds) / count
    mean_cube = sum(speed ** 3 for speed in speeds) / count
    fits = {'max_likelihood': max_likelihood(speeds), 'least_squares': least_squares(speeds)}
    pattern = mean_cube / mean ** 3
    shape = bisect(lambda k: math.gamma(1 + 3 / k) / math.gamma(1 + 1 / k) ** 3 - pattern, *SHAPE_BRACKET)
    fits['openwind'] = (shape, mean / math.gamma(1 + 1 / shape))
    above = sum(1 for speed in speeds if speed > mean) / count

    def scale_for(shape):
        return (mean_cube / math.gamma(1 + 3 / shape)) ** (1 / 3)

    shape = bisect(lambda k: math.exp(-((mean / scale_for(k)) ** k)) - above, *SHAPE_BRACKET)
    fits['wasp'] = (shape, scale_for(shape))
    return mean, mean_cube, fits


def sector(direction, sectors):
    width = DEGREES_PER_TURN / sectors
    return int(((direction % DEGREES_PER_TURN) + width / 2) // width) % sectors


def gumbel_max_likelihood(sample):
    """Gumbel (mode, scale) by maximum likelihood: the scale solves
    β = mean − Σx·e^(−x/β) / Σe^(−x/β), by bisection."""
    mean, smallest = statistics.fmean(sample), min(sample)

    def equation(scale):
        weights = [math.exp(-(value - smallest) / scale) for value in sample]
        return mean - scale - sum(value * weight for value, weight in zip(sample, weights)) / sum(weights)

    low, high = 1e-3, 1e3
    for _ in range(BISECTION_STEPS):
        middle = (low + high) / 2
        if (equation(middle) > 0) == (equation(low) > 0):
            low = middle
        else:
            high = middle
    scale = (low + high) / 2
    mode = smallest - scale * math.log(statistics.fmean([math.exp(-(value - smallest) / scale) for value in sample]))
    return mode, scale


def wind(binary, sample, *arguments):
    """One `wind analyze` answer, as JSON."""
    run = subprocess.run([binary, 'analyze', sample, *arguments], capture_output=True, text=True, encoding='utf-8')
    if run.returncode != 0:
        sys.exit(f'wind {arguments}: {run.stderr}')
    return json.loads(run.stdout)


failures = []


def check(label, ours, theirs, tolerance, absolute=False):
    gap = abs(ours - theirs) if absolute else abs(ours - theirs) / max(abs(ours), abs(theirs), 1e-300)
    status = 'ok' if gap <= tolerance else 'DIVERGE'
    print(f'{status:8} {label:42} independent {ours:.10g}   core {theirs:.10g}')
    if status != 'ok':
        failures.append(label)


def main(sample, binary):
    # Labels use α and ³; a Windows console defaults to cp1252.
    sys.stdout.reconfigure(encoding='utf-8')
    if not os.path.exists(binary) and os.path.exists(binary + '.exe'):
        binary += '.exe'
    # Windows resolves a relative program path against the system search path, not the working directory.
    binary = os.path.abspath(binary)
    rows = read(sample)
    speeds = [values[SPEED_TOP] for _, values in rows if values[SPEED_TOP] is not None]
    mean, mean_cube, fits = weibull_fits(speeds)
    for method, (shape, scale) in fits.items():
        answer = wind(binary, sample, 'histogram', f'column={SPEED_TOP}', 'binWidth=1', f'weibull={method}')
        if method == 'max_likelihood':
            check('samples', len(speeds), answer['samples'], 0, absolute=True)
            check('mean speed', mean, answer['mean'], SUM_TOLERANCE)
            check('power density at 1.225 kg/m³', HALF * RHO_STD * mean_cube, answer['powerDensityWM2'], SUM_TOLERANCE)
            check('energy pattern factor', mean_cube / mean ** 3, answer['energyPatternFactor'], SUM_TOLERANCE)
        check(f'Weibull {method} k', shape, answer['weibull']['k'], FIT_TOLERANCE)
        check(f'Weibull {method} A', scale, answer['weibull']['a'], FIT_TOLERANCE)
    distribution = wind(binary, sample, 'distribution', f'column={SPEED_TOP}')
    check('distribution share above the mean', sum(1 for speed in speeds if speed > mean) / len(speeds), distribution['aboveMean'], SUM_TOLERANCE)
    for row in distribution['fits']:
        shape, scale = fits[row['method']]
        check(f"distribution {row['method']} R²", distribution_r_squared(speeds, shape, scale), row['rSquared'], FIT_TOLERANCE)

    # Shear between the two mean speeds: AP-Mean and AP-MoMM.
    both = [(stamp, values[SPEED_TOP], values[SPEED_BOTTOM]) for stamp, values in rows
            if values[SPEED_TOP] is not None and values[SPEED_BOTTOM] is not None]
    log_ratio = math.log(HEIGHT_TOP_M / HEIGHT_BOTTOM_M)
    simple = math.log(sum(top for _, top, _ in both) / sum(bottom for _, _, bottom in both)) / log_ratio
    by_month = defaultdict(lambda: [0, 0.0, 0.0])
    for stamp, top, bottom in both:
        cell = by_month[stamp.month - 1]
        cell[0] += 1
        cell[1] += top
        cell[2] += bottom
    weights = {month: min(1.0, count / (STEPS_PER_DAY * MEAN_DAYS[month])) * MEAN_DAYS[month] for month, (count, _, _) in by_month.items()}

    def momm(column):
        return sum(weights[m] * cell[column] / cell[0] for m, cell in by_month.items()) / sum(weights.values())

    seasonal = math.log(momm(1) / momm(2)) / log_ratio
    columns = json.dumps([SPEED_TOP, SPEED_BOTTOM], ensure_ascii=False)
    check('shear α, AP-Mean', simple, wind(binary, sample, 'shear', f'columns={columns}', 'sectors=12', 'aggregation=ap_mean')['alpha'], SUM_TOLERANCE)
    check('shear α, AP-MoMM (default)', seasonal, wind(binary, sample, 'shear', f'columns={columns}', 'sectors=12')['alpha'], SUM_TOLERANCE)

    # Rose: frequency per sector of the steps with direction and speed.
    pairs = [values[DIRECTION] for _, values in rows if values[DIRECTION] is not None and values[SPEED_TOP] is not None]
    counts = [0] * ROSE_SECTORS
    for direction in pairs:
        counts[sector(direction, ROSE_SECTORS)] += 1
    rose = wind(binary, sample, 'rose', f'direction={DIRECTION}', f'speed={SPEED_TOP}', f'sectors={ROSE_SECTORS}')
    for index, count in enumerate(counts):
        check(f'rose sector {index} frequency', count / len(pairs), rose['frequency'][index] or 0.0, SHARE_ABSOLUTE, absolute=True)

    # Diurnal mean by hour of the step's start.
    hourly = defaultdict(list)
    for stamp, values in rows:
        if values[SPEED_TOP] is not None:
            hourly[stamp.hour].append(values[SPEED_TOP])
    diurnal = wind(binary, sample, 'diurnal', f'column={SPEED_TOP}')
    for hour in DIURNAL_HOURS_CHECKED:
        check(f'diurnal mean at {hour:02d}h', sum(hourly[hour]) / len(hourly[hour]), diurnal['all'][hour], SUM_TOLERANCE)

    # Coverage: valid steps over the steps of the regular grid.
    first, last = rows[0][0], rows[-1][0]
    grid_steps = int((last - first).total_seconds()) // STEP_S + 1
    coverage = wind(binary, sample, 'coverage')
    check('coverage of the top speed', len(speeds) / grid_steps, coverage['total'][0], SUM_TOLERANCE)

    # Turbulence: the 15 m/s bin of the top speed and its IEC category.
    at_15 = [values[SPEED_TOP_SD] / values[SPEED_TOP] for _, values in rows
             if values[SPEED_TOP] is not None and values[SPEED_TOP_SD] is not None
             and values[SPEED_TOP] >= TI_MIN_SPEED and BIN_15[0] <= values[SPEED_TOP] < BIN_15[1]]
    mean_ti, deviation_ti = statistics.fmean(at_15), statistics.stdev(at_15)
    representative = mean_ti + QUANTILE_90 * deviation_ti
    category = next((name for name, reference in CATEGORIES
                     if representative <= reference * (NTM_SLOPE + NTM_OFFSET / REFERENCE_SPEED)), 'S')
    speeds_json = json.dumps([SPEED_TOP], ensure_ascii=False)
    turbulence = wind(binary, sample, 'turbulence', f'speeds={speeds_json}', 'versus={"kind":"height"}')['at15'][0]
    check('TI steps at 15 m/s', len(at_15), turbulence['count'], 0, absolute=True)
    check('TI mean at 15 m/s', mean_ti, turbulence['mean'], SUM_TOLERANCE)
    check('TI standard deviation at 15 m/s', deviation_ti, turbulence['deviation'], DEVIATION_TOLERANCE)
    check('representative TI at 15 m/s', representative, turbulence['figure'], DEVIATION_TOLERANCE)
    same_category = category == turbulence['category']
    print(f"{'ok' if same_category else 'DIVERGE':8} {'IEC turbulence category':42} independent {category}   core {turbulence['category']}")
    if not same_category:
        failures.append('IEC turbulence category')

    # Extremes: the 50-year wind from monthly maxima.
    monthly = defaultdict(list)
    for stamp, values in rows:
        if values[SPEED_TOP] is not None:
            monthly[(stamp.year, stamp.month)].append(values[SPEED_TOP])
    maxima = [max(found) for (year, month), found in sorted(monthly.items())
              if len(found) >= MIN_RECOVERY * calendar.monthrange(year, month)[1] * STEPS_PER_DAY]
    mode, scale = gumbel_max_likelihood(maxima)
    annual_mode = mode + scale * math.log(MONTHS_PER_YEAR)
    design = annual_mode - scale * math.log(-math.log(1.0 - 1.0 / DESIGN_YEARS))
    extremes = wind(binary, sample, 'extremes', f'column={SPEED_TOP}', 'method={"kind":"periodic_maxima","period":"month"}')
    check('monthly maxima', len(maxima), len(extremes['peaks']), 0, absolute=True)
    check('Gumbel scale β (maximum likelihood)', scale, extremes['fit']['scale'], FIT_TOLERANCE)
    check('Gumbel mode μ', mode, extremes['fit']['mode'], FIT_TOLERANCE)
    check('50-year wind speed', design, extremes['design'], FIT_TOLERANCE)

    # Monthly means table: the MoMM weighs each month's mean by its mean days.
    month_means = [statistics.fmean(found) for (_, month), found in sorted(monthly.items())]
    momm_table = sum(mean * days for mean, days in zip(month_means, MEAN_DAYS)) / sum(MEAN_DAYS)
    check('mean of monthly means (MoMM)', momm_table, wind(binary, sample, 'table', f'column={SPEED_TOP}')['momm'], SUM_TOLERANCE)

    # Scatter: least squares line of the top speed on the bottom one.
    line = statistics.linear_regression([bottom for _, _, bottom in both], [top for _, top, _ in both])
    scatter = wind(binary, sample, 'scatter', f'x={SPEED_BOTTOM}', f'y={SPEED_TOP}')
    check('scatter slope', line.slope, scatter['slope'], SUM_TOLERANCE)
    check('scatter intercept', line.intercept, scatter['intercept'], FIT_TOLERANCE, absolute=True)

    print()
    if failures:
        sys.exit(f'{len(failures)} figure(s) diverge: {", ".join(failures)}')
    print('crosscheck: every figure matches the independent calculation')


if __name__ == '__main__':
    main(*sys.argv[1:3])

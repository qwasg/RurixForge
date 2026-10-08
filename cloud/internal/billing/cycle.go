package billing

import "time"

// 用量周期（15_CLOUD_SERVICE.md §11.1）：档位订阅按月滚动（锚定订阅开始时刻），额度包整段有效期一个周期，
// Hobby 免费额度与没有档位订阅的用户用 UTC 自然月。

const (
	cycleMonth  = "month"
	cyclePeriod = "period"
)

// addMonths 返回 t 加 n 个自然月（UTC）；目标月没有该日时取月末（1/31 + 1 个月 = 2/28 或 2/29）。
func addMonths(t time.Time, n int) time.Time {
	t = t.UTC()
	y, m, d := t.Date()
	first := time.Date(y, m+time.Month(n), 1, 0, 0, 0, 0, time.UTC)
	if last := first.AddDate(0, 1, -1).Day(); d > last {
		d = last
	}
	return time.Date(first.Year(), first.Month(), d, t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), time.UTC)
}

// monthlyCycle 返回 now 所在的按月用量周期 [start, end)，end 不超过订阅结束时间。
// now 早于订阅开始（已预约的订阅）时返回第一个周期。
func monthlyCycle(startsAt, endsAt, now time.Time) (time.Time, time.Time) {
	s := startsAt.UTC()
	now = now.UTC()
	n := 0
	if now.After(s) {
		n = (now.Year()-s.Year())*12 + int(now.Month()) - int(s.Month())
		for n > 0 && addMonths(s, n).After(now) {
			n--
		}
		for !addMonths(s, n+1).After(now) {
			n++
		}
	}
	start, end := addMonths(s, n), addMonths(s, n+1)
	if e := endsAt.UTC(); end.After(e) {
		end = e
	}
	return start, end
}

// calendarMonth 返回 now 所在的 UTC 自然月 [start, end)。
func calendarMonth(now time.Time) (time.Time, time.Time) {
	y, m, _ := now.UTC().Date()
	start := time.Date(y, m, 1, 0, 0, 0, 0, time.UTC)
	return start, start.AddDate(0, 1, 0)
}

// usageCycleOf 返回订阅在 now 的用量周期。
func usageCycleOf(cycle string, startsAt, endsAt, now time.Time) (time.Time, time.Time) {
	if cycle == cycleMonth {
		return monthlyCycle(startsAt, endsAt, now)
	}
	return startsAt.UTC(), endsAt.UTC()
}

// termMonths 是付费周期对应的期限（按月 1 个自然月，按年 12 个）。
func termMonths(interval string) int {
	if interval == IntervalYear {
		return 12
	}
	return 1
}

func sameDate(a, b time.Time) bool {
	ay, am, ad := a.Date()
	by, bm, bd := b.Date()
	return ay == by && am == bm && ad == bd
}

func dateOnly(t time.Time) time.Time {
	y, m, d := t.UTC().Date()
	return time.Date(y, m, d, 0, 0, 0, 0, time.UTC)
}

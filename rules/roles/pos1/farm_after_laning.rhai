// Example role rule: as the carry, start thinking about farm patterns once
// laning ends. Replace or extend with your own pos 1 habits.
fn check(s) {
    if s.clock_time >= 600 && s.clock_time < 605 {
        #{ say: "Laning is ending. Plan your farming route.", priority: 1, cooldown: 600 }
    }
}

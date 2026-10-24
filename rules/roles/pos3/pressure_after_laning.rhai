// Example role rule: as the offlaner, look to make space once laning ends.
fn check(s) {
    if s.clock_time >= 600 && s.clock_time < 605 {
        #{ say: "Laning is ending. Look to pressure a tower or join your team.", priority: 1, cooldown: 600 }
    }
}

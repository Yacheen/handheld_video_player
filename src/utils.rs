pub fn format_timecode(current_frames: u64, total_frames: u64, fps: u64) -> String {
    fn to_time_parts(total_seconds: u64) -> (u64, u64, u64) {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        (hours, minutes, seconds)
    }

    let current_seconds = current_frames / fps;
    let total_seconds = total_frames / fps;

    let (cur_h, cur_m, cur_s) = to_time_parts(current_seconds);
    let (tot_h, tot_m, tot_s) = to_time_parts(total_seconds);

    if tot_h > 0 {
        format!(
            "{:01}:{:02}:{:02} / {:01}:{:02}:{:02}",
            cur_h, cur_m, cur_s, tot_h, tot_m, tot_s
        )
    } else {
        format!(
            "{:01}:{:02} / {:01}:{:02}",
            cur_m, cur_s, tot_m, tot_s
        )
    }
}

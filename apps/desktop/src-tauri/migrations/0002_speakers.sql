-- 1-based speaker id from diarization; NULL when diarization was off or no
-- speaker turn overlapped the segment.
ALTER TABLE segments ADD COLUMN speaker INTEGER;

SET target_folder=C:\Users\frede\Documents\VM\Shared\in\CLionProjects
SET temp_source_folder=C:\Users\frede\CLionProjects

SET temp_source_to_copy=google_bigquery\
xcopy %temp_source_folder%\%temp_source_to_copy% %target_folder%\%temp_source_to_copy% /exclude:%temp_source_folder%\exclude_in_copy.txt /e /y
SET temp_source_to_copy=tests\twitch_backup\
xcopy %temp_source_folder%\%temp_source_to_copy% %target_folder%\%temp_source_to_copy% /exclude:%temp_source_folder%\exclude_in_copy.txt /e /y
SET temp_source_to_copy=exponential_backoff\
xcopy %temp_source_folder%\%temp_source_to_copy% %target_folder%\%temp_source_to_copy% /exclude:%temp_source_folder%\exclude_in_copy.txt /e /y
echo DONE

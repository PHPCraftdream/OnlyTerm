# update_all function

{{since('20230320-124340-559cb7b0')}}

!!! Warning

    Git-based plugin installation has been removed from wezterm. There is no
    longer a managed clone directory for `update_all()` to fetch/fast-forward,
    so calling this function now raises an error explaining the removal.
    Update a plugin by updating its files in its local directory yourself
    (for example, running `git pull` there), then reload your configuration.


<!--
 * @Author: Rais
 * @Date: 2026-04-22 14:35:03
 * @LastEditTime: 2026-04-22 14:35:04
 * @LastEditors: Rais
 * @Description:
-->
  隐藏 启动

  rdog hidden-daemon -c "C:\path\to\rdog_unity.toml"
  [hidden]

  log_file = 'C:\logs\rdog_hidden.log'

  然后这样看是否启动成功:

  Get-Process rdog -ErrorAction Stop
  Get-Content C:\logs\rdog_hidden.log -Tail 50

// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

import React, { Component, PropTypes } from 'react';
import { connect } from 'react-redux';
import { bindActionCreators } from 'redux';
import ContentAdd from 'material-ui/svg-icons/content/add';

import List from './List';
import { CreateAccount } from '../../modals';
import { Actionbar, Button, Page, Tooltip } from '../../ui';

import styles from './accounts.css';

class Accounts extends Component {
  static contextTypes = {
    api: PropTypes.object
  }

  static propTypes = {
    accounts: PropTypes.object,
    hasAccounts: PropTypes.bool,
    balances: PropTypes.object
  }

  state = {
    addressBook: false,
    newDialog: false
  }

  render () {
    const { accounts, hasAccounts, balances } = this.props;

    return (
      <div className={ styles.accounts }>
        { this.renderNewDialog() }
        { this.renderActionbar() }
        <Page>
          <List
            accounts={ accounts }
            balances={ balances }
            empty={ !hasAccounts } />
          <Tooltip
            className={ styles.accountTooltip }
            text='your accounts are visible for easy access, allowing you to edit the meta information, make transfers, view transactions and fund the account' />
        </Page>
      </div>
    );
  }

  renderActionbar () {
    const buttons = [
      <Button
        key='newAccount'
        icon={ <ContentAdd /> }
        label='new account'
        onClick={ this.onNewAccountClick } />
    ];

    return (
      <Actionbar
        className={ styles.toolbar }
        title='Accounts Overview'
        buttons={ buttons }>
        <Tooltip
          className={ styles.toolbarTooltip }
          right
          text='actions relating to the current view are available on the toolbar for quick access, be it for performing actions or creating a new item' />
      </Actionbar>
    );
  }

  renderNewDialog () {
    const { accounts } = this.props;
    const { newDialog } = this.state;

    if (!newDialog) {
      return null;
    }

    return (
      <CreateAccount
        accounts={ accounts }
        onClose={ this.onNewAccountClose }
        onUpdate={ this.onNewAccountUpdate } />
    );
  }

  onNewAccountClick = () => {
    this.setState({
      newDialog: !this.state.newDialog
    });
  }

  onNewAccountClose = () => {
    this.onNewAccountClick();
  }

  onNewAccountUpdate = () => {
  }
}

function mapStateToProps (state) {
  const { accounts, hasAccounts } = state.personal;
  const { balances } = state.balances;

  return {
    accounts,
    hasAccounts,
    balances
  };
}

function mapDispatchToProps (dispatch) {
  return bindActionCreators({}, dispatch);
}

export default connect(
  mapStateToProps,
  mapDispatchToProps
)(Accounts);

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

import List from '../Accounts/List';
import { AddAddress } from '../../modals';
import { Actionbar, Button, Page } from '../../ui';

import styles from './addresses.css';

class Addresses extends Component {
  static contextTypes = {
    api: PropTypes.object
  }

  static propTypes = {
    balances: PropTypes.object,
    contacts: PropTypes.object,
    hasContacts: PropTypes.bool
  }

  state = {
    showAdd: false
  }

  render () {
    const { balances, contacts, hasContacts } = this.props;

    return (
      <div className={ styles.addresses }>
        { this.renderActionbar() }
        { this.renderAddAddress() }
        <Page>
          <List
            link='address'
            accounts={ contacts }
            balances={ balances }
            empty={ !hasContacts } />
        </Page>
      </div>
    );
  }

  renderActionbar () {
    const buttons = [
      <Button
        key='newAddress'
        icon={ <ContentAdd /> }
        label='new address'
        onClick={ this.onOpenAdd } />
    ];

    return (
      <Actionbar
        className={ styles.toolbar }
        title='Saved Addresses'
        buttons={ buttons } />
    );
  }

  renderAddAddress () {
    const { contacts } = this.props;
    const { showAdd } = this.state;

    if (!showAdd) {
      return null;
    }

    return (
      <AddAddress
        contacts={ contacts }
        onClose={ this.onCloseAdd } />
    );
  }

  onOpenAdd = () => {
    this.setState({
      showAdd: true
    });
  }

  onCloseAdd = () => {
    this.setState({ showAdd: false });
  }
}

function mapStateToProps (state) {
  const { balances } = state.balances;
  const { contacts, hasContacts } = state.personal;

  return {
    balances,
    contacts,
    hasContacts
  };
}

function mapDispatchToProps (dispatch) {
  return bindActionCreators({}, dispatch);
}

export default connect(
  mapStateToProps,
  mapDispatchToProps
)(Addresses);
